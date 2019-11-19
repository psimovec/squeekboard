/**! The parsing of the data files for layouts */

use std::cell::RefCell;
use std::collections::{ HashMap, HashSet };
use std::env;
use std::ffi::CString;
use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;
use std::vec::Vec;

use xkbcommon::xkb;

use ::keyboard::{
    KeyState, PressType,
    generate_keymap, generate_keycodes, FormattingError
};
use ::layout::ArrangementKind;
use ::resources;
use ::util::c::as_str;
use ::util::hash_map_map;
use ::xdg;

// traits, derives
use std::io::BufReader;
use std::iter::FromIterator;
use serde::Deserialize;
use util::WarningHandler;


/// Gathers stuff defined in C or called by C
pub mod c {
    use super::*;
    use std::os::raw::c_char;

    #[no_mangle]
    pub extern "C"
    fn squeek_load_layout(
        name: *const c_char,
        type_: u32,
    ) -> *mut ::layout::Layout {
        let type_ = match type_ {
            0 => ArrangementKind::Base,
            1 => ArrangementKind::Wide,
            _ => panic!("Bad enum value"),
        };
        let name = as_str(&name)
            .expect("Bad layout name")
            .expect("Empty layout name");

        let (kind, layout) = load_layout_data_with_fallback(&name, type_);
        let layout = ::layout::Layout::new(layout, kind);
        Box::into_raw(Box::new(layout))
    }
}

const FALLBACK_LAYOUT_NAME: &str = "us";

#[derive(Debug)]
pub enum LoadError {
    BadData(Error),
    MissingResource,
    BadResource(serde_yaml::Error),
    BadKeyMap(FormattingError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use self::LoadError::*;
        match self {
            BadData(e) => write!(f, "Bad data: {}", e),
            MissingResource => write!(f, "Missing resource"),
            BadResource(e) => write!(f, "Bad resource: {}", e),
            BadKeyMap(e) => write!(f, "Bad key map: {}", e),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum DataSource {
    File(PathBuf),
    Resource(String),
}

impl fmt::Display for DataSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataSource::File(path) => write!(f, "Path: {:?}", path.display()),
            DataSource::Resource(name) => write!(f, "Resource: {}", name),
        }
    }
}

/// Lists possible sources, with 0 as the most preferred one
/// Trying order: native lang of the right kind, native base,
/// fallback lang of the right kind, fallback base
fn list_layout_sources(
    name: &str,
    kind: ArrangementKind,
    keyboards_path: Option<PathBuf>,
) -> Vec<(ArrangementKind, DataSource)> {
    let mut ret = Vec::new();
    {
        fn name_with_arrangement(name: String, kind: &ArrangementKind)
            -> String
        {
            match kind {    
                ArrangementKind::Base => name,
                ArrangementKind::Wide => name + "_wide",
            }
        }

        let mut add_by_name = |name: &str, kind: &ArrangementKind| {
            if let Some(path) = keyboards_path.clone() {
                ret.push((
                    kind.clone(),
                    DataSource::File(
                        path.join(name.to_owned()).with_extension("yaml")
                    )
                ))
            }
            
            ret.push((
                kind.clone(),
                DataSource::Resource(name.into())
            ));
        };

        match &kind {
            ArrangementKind::Base => {},
            kind => add_by_name(
                &name_with_arrangement(name.into(), &kind),
                &kind,
            ),
        };

        add_by_name(name, &ArrangementKind::Base);

        match &kind {
            ArrangementKind::Base => {},
            kind => add_by_name(
                &name_with_arrangement(FALLBACK_LAYOUT_NAME.into(), &kind),
                &kind,
            ),
        };

        add_by_name(FALLBACK_LAYOUT_NAME, &ArrangementKind::Base);
    }
    ret
}

struct PrintWarnings;

impl WarningHandler for PrintWarnings {
    fn handle(&mut self, warning: &str) {
        println!("{}", warning);
    }
}

fn load_layout_data(source: DataSource)
    -> Result<::layout::LayoutData, LoadError>
{
    let handler = PrintWarnings{};
    match source {
        DataSource::File(path) => {
            Layout::from_file(path.clone())
                .map_err(LoadError::BadData)
                .and_then(|layout|
                    layout.build(handler).0.map_err(LoadError::BadKeyMap)
                )
        },
        DataSource::Resource(name) => {
            Layout::from_resource(&name)
                .and_then(|layout|
                    layout.build(handler).0.map_err(LoadError::BadKeyMap)
                )
        },
    }
}

fn load_layout_data_with_fallback(
    name: &str,
    kind: ArrangementKind,
) -> (ArrangementKind, ::layout::LayoutData) {
    let path = env::var_os("SQUEEKBOARD_KEYBOARDSDIR")
        .map(PathBuf::from)
        .or_else(|| xdg::data_path("squeekboard/keyboards"));
    
    for (kind, source) in list_layout_sources(name, kind, path) {
        let layout = load_layout_data(source.clone());
        match layout {
            Err(e) => match (e, source) {
                (
                    LoadError::BadData(Error::Missing(e)),
                    DataSource::File(file)
                ) => eprintln!( // TODO: print in debug logging level
                    "Tried file {:?}, but it's missing: {}",
                    file, e
                ),
                (e, source) => eprintln!(
                    "Failed to load layout from {}: {}, skipping",
                    source, e
                ),
            },
            Ok(layout) => return (kind, layout),
        }
    }

    panic!("No useful layout found!");
}

/// The root element describing an entire keyboard
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    bounds: Bounds,
    views: HashMap<String, Vec<ButtonIds>>,
    #[serde(default)] 
    buttons: HashMap<String, ButtonMeta>,
    outlines: HashMap<String, Outline>
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Bounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Buttons are embedded in a single string
type ButtonIds = String;

/// All info about a single button
/// Buttons can have multiple instances though.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ButtonMeta {
    /// Special action to perform on activation. Conflicts with keysym, text.
    action: Option<Action>,
    /// The name of the XKB keysym to emit on activation.
    /// Conflicts with action, text
    keysym: Option<String>,
    /// The text to submit on activation. Will be derived from ID if not present
    /// Conflicts with action, keysym
    text: Option<String>,
    /// If not present, will be derived from text or the button ID
    label: Option<String>,
    /// Conflicts with label
    icon: Option<String>,
    /// The name of the outline. If not present, will be "default"
    outline: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
enum Action {
    #[serde(rename="locking")]
    Locking { lock_view: String, unlock_view: String },
    #[serde(rename="set_view")]
    SetView(String),
    #[serde(rename="show_prefs")]
    ShowPrefs,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Outline {
    bounds: Bounds,
}

/// Errors encountered loading the layout into yaml
#[derive(Debug)]
pub enum Error {
    Yaml(serde_yaml::Error),
    Io(io::Error),
    /// The file was missing.
    /// It's distinct from Io in order to make it matchable
    /// without calling io::Error::kind()
    Missing(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Yaml(e) => write!(f, "YAML: {}", e),
            Error::Io(e) => write!(f, "IO: {}", e),
            Error::Missing(e) => write!(f, "Missing: {}", e),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        let kind = e.kind();
        match kind {
            io::ErrorKind::NotFound => Error::Missing(e),
            _ => Error::Io(e),
        }
    }
}

impl Layout {
    pub fn from_resource(name: &str) -> Result<Layout, LoadError> {
        let data = resources::get_keyboard(name)
                    .ok_or(LoadError::MissingResource)?;
        serde_yaml::from_str(data)
                    .map_err(LoadError::BadResource)
    }

    pub fn from_file(path: PathBuf) -> Result<Layout, Error> {
        let infile = BufReader::new(
            fs::OpenOptions::new()
                .read(true)
                .open(&path)?
        );
        serde_yaml::from_reader(infile).map_err(Error::Yaml)
    }

    pub fn build<H: WarningHandler>(self, mut warning_handler: H)
        -> (Result<::layout::LayoutData, FormattingError>, H)
    {
        let button_names = self.views.values()
            .flat_map(|rows| {
                rows.iter()
                    .flat_map(|row| row.split_ascii_whitespace())
            });
        
        let button_names: HashSet<&str>
            = HashSet::from_iter(button_names);

        let button_actions: Vec<(&str, ::action::Action)>
            = button_names.iter().map(|name| {(
                *name,
                create_action(
                    &self.buttons,
                    name,
                    self.views.keys().collect(),
                    &mut warning_handler,
                )
            )}).collect();

        let keymap: HashMap<String, u32> = generate_keycodes(
            button_actions.iter()
                .filter_map(|(_name, action)| {
                    match action {
                        ::action::Action::Submit {
                            text: _, keys,
                        } => Some(keys),
                        _ => None,
                    }
                })
                .flatten()
                .map(|named_keysym| named_keysym.0.as_str())
        );

        let button_states = button_actions.into_iter().map(|(name, action)| {
            let keycodes = match &action {
                ::action::Action::Submit { text: _, keys } => {
                    keys.iter().map(|named_keycode| {
                        *keymap.get(named_keycode.0.as_str())
                            .expect(
                                format!(
                                    "keycode {} in key {} missing from keymap",
                                    named_keycode.0,
                                    name
                                ).as_str()
                            )
                    }).collect()
                },
                _ => Vec::new(),
            };
            (
                name.into(),
                KeyState {
                    pressed: PressType::Released,
                    locked: false,
                    keycodes,
                    action,
                }
            )
        });

        let button_states = HashMap::<String, KeyState>::from_iter(
            button_states
        );

        // TODO: generate from symbols
        let keymap_str = match generate_keymap(&button_states) {
            Err(e) => { return (Err(e), warning_handler) },
            Ok(v) => v,
        };

        let button_states_cache = hash_map_map(
            button_states,
            |name, state| {(
                name,
                Rc::new(RefCell::new(state))
            )}
        );

        let views = HashMap::from_iter(
            self.views.iter().map(|(name, view)| {(
                name.clone(),
                Box::new(::layout::View {
                    bounds: ::layout::c::Bounds {
                        x: self.bounds.x,
                        y: self.bounds.y,
                        width: self.bounds.width,
                        height: self.bounds.height,
                    },
                    rows: view.iter().map(|row| {
                        Box::new(::layout::Row {
                            angle: 0,
                            bounds: None,
                            buttons: row.split_ascii_whitespace().map(|name| {
                                Box::new(create_button(
                                    &self.buttons,
                                    &self.outlines,
                                    name,
                                    button_states_cache.get(name.into())
                                        .expect("Button state not created")
                                        .clone(),
                                    &mut warning_handler,
                                ))
                            }).collect(),
                        })
                    }).collect(),
                })
            )})
        );

        (
            Ok(::layout::LayoutData {
                views: views,
                keymap_str: {
                    CString::new(keymap_str)
                        .expect("Invalid keymap string generated")
                },
            }),
            warning_handler,
        )
    }
}

fn create_action<H: WarningHandler>(
    button_info: &HashMap<String, ButtonMeta>,
    name: &str,
    view_names: Vec<&String>,
    warning_handler: &mut H,
) -> ::action::Action {
    let default_meta = ButtonMeta::default();
    let symbol_meta = button_info.get(name)
        .unwrap_or(&default_meta);

    fn keysym_valid(name: &str) -> bool {
        xkb::keysym_from_name(name, xkb::KEYSYM_NO_FLAGS) != xkb::KEY_NoSymbol
    }
    
    enum SubmitData {
        Action(Action),
        Text(String),
        Keysym(String),
    };
    
    let submission = match (&symbol_meta.action, &symbol_meta.keysym, &symbol_meta.text) {
        (Some(action), None, None) => SubmitData::Action(action.clone()),
        (None, Some(keysym), None) => SubmitData::Keysym(keysym.clone()),
        (None, None, Some(text)) => SubmitData::Text(text.clone()),
        (None, None, None) => SubmitData::Text(name.into()),
        _ => {
            warning_handler.handle(&format!(
                "Button {} has more than one of (action, keysym, text)",
                name
            ));
            SubmitData::Text("".into())
        },
    };

    fn filter_view_name<H: WarningHandler>(
        button_name: &str,
        view_name: String,
        view_names: &Vec<&String>,
        warning_handler: &mut H,
    ) -> String {
        if view_names.contains(&&view_name) {
            view_name
        } else {
            warning_handler.handle(&format!("Button {} switches to missing view {}",
                button_name,
                view_name,
            ));
            "base".into()
        }
    }

    type SD = SubmitData;

    match submission {
        SD::Action(Action::SetView(view_name)) => ::action::Action::SetLevel(
            filter_view_name(
                name, view_name.clone(), &view_names,
                warning_handler,
            )
        ),
        SD::Action(Action::Locking {
            lock_view, unlock_view
        }) => ::action::Action::LockLevel {
            lock: filter_view_name(
                name,
                lock_view.clone(),
                &view_names,
                warning_handler,
            ),
            unlock: filter_view_name(
                name,
                unlock_view.clone(),
                &view_names,
                warning_handler,
            ),
        },
        SD::Action(Action::ShowPrefs) => ::action::Action::ShowPreferences,
        SD::Keysym(keysym) => ::action::Action::Submit {
            text: None,
            keys: vec!(::action::KeySym(
                match keysym_valid(keysym.as_str()) {
                    true => keysym.clone(),
                    false => {
                        warning_handler.handle(&format!(
                            "Keysym name invalid: {}",
                            keysym,
                        ));
                        "space".into() // placeholder
                    },
                }
            )),
        },
        SD::Text(text) => ::action::Action::Submit {
            text: {
                CString::new(text.clone())
                    .map_err(|e| {
                        warning_handler.handle(&format!(
                            "Text {} contains problems: {:?}",
                            text,
                            e
                        ));
                        e
                    }).ok()
            },
            keys: text.chars().map(|codepoint| {
                let codepoint_string = codepoint.to_string();
                ::action::KeySym(match keysym_valid(codepoint_string.as_str()) {
                    true => codepoint_string,
                    false => format!("U{:04X}", codepoint as u32),
                })
            }).collect(),
        }
    }
}

/// TODO: Since this will receive user-provided data,
/// all .expect() on them should be turned into soft fails
fn create_button<H: WarningHandler>(
    button_info: &HashMap<String, ButtonMeta>,
    outlines: &HashMap<String, Outline>,
    name: &str,
    state: Rc<RefCell<KeyState>>,
    warning_handler: &mut H,
) -> ::layout::Button {
    let cname = CString::new(name.clone())
        .expect("Bad name");
    // don't remove, because multiple buttons with the same name are allowed
    let default_meta = ButtonMeta::default();
    let button_meta = button_info.get(name)
        .unwrap_or(&default_meta);

    // TODO: move conversion to the C/Rust boundary
    let label = if let Some(label) = &button_meta.label {
        ::layout::Label::Text(CString::new(label.as_str())
            .expect("Bad label"))
    } else if let Some(icon) = &button_meta.icon {
        ::layout::Label::IconName(CString::new(icon.as_str())
            .expect("Bad icon"))
    } else if let Some(text) = &button_meta.text {
        ::layout::Label::Text(
            CString::new(text.as_str())
                .unwrap_or_else(|e| {
                    warning_handler.handle(&format!(
                        "Text {} is invalid: {}",
                        text,
                        e,
                    ));
                    CString::new("").unwrap()
                })
        )
    } else {
        ::layout::Label::Text(cname.clone())
    };

    let outline_name = match &button_meta.outline {
        Some(outline) => {
            if outlines.contains_key(outline) {
                outline.clone()
            } else {
                warning_handler.handle(&format!("Outline named {} does not exist! Using default for button {}", outline, name));
                "default".into()
            }
        }
        None => "default".into(),
    };

    let outline = outlines.get(&outline_name)
        .map(|outline| (*outline).clone())
        .unwrap_or_else(|| {
            warning_handler.handle(
                &format!("No default outline defined! Using 1x1!")
            );
            Outline {
                bounds: Bounds { x: 0f64, y: 0f64, width: 1f64, height: 1f64 },
            }
        });

    ::layout::Button {
        name: cname,
        outline_name: CString::new(outline_name).expect("Bad outline"),
        // TODO: do layout before creating buttons
        bounds: ::layout::c::Bounds {
            x: outline.bounds.x,
            y: outline.bounds.y,
            width: outline.bounds.width,
            height: outline.bounds.height,
        },
        label: label,
        state: state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    use std::error::Error as ErrorTrait;

    struct PanicWarn;
    
    impl WarningHandler for PanicWarn {
        fn handle(&mut self, warning: &str) {
            panic!("{}", warning);
        }
    }

    #[test]
    fn test_parse_path() {
        assert_eq!(
            Layout::from_file(PathBuf::from("tests/layout.yaml")).unwrap(),
            Layout {
                bounds: Bounds { x: 0f64, y: 0f64, width: 0f64, height: 0f64 },
                views: hashmap!(
                    "base".into() => vec!("test".into()),
                ),
                buttons: hashmap!{
                    "test".into() => ButtonMeta {
                        icon: None,
                        keysym: None,
                        action: None,
                        text: None,
                        label: Some("test".into()),
                        outline: None,
                    }
                },
                outlines: hashmap!{
                    "default".into() => Outline {
                        bounds: Bounds {
                            x: 0f64, y: 0f64, width: 0f64, height: 0f64
                        }, 
                    }
                },
            }
        );
    }

    /// Check if the default protection works
    #[test]
    fn test_empty_views() {
        let out = Layout::from_file(PathBuf::from("tests/layout2.yaml"));
        match out {
            Ok(_) => assert!(false, "Data mistakenly accepted"),
            Err(e) => {
                let mut handled = false;
                if let Error::Yaml(ye) = &e {
                    handled = ye.description() == "missing field `views`";
                };
                if !handled {
                    println!("Unexpected error {:?}", e);
                    assert!(false)
                }
            }
        }
    }

    #[test]
    fn test_extra_field() {
        let out = Layout::from_file(PathBuf::from("tests/layout3.yaml"));
        match out {
            Ok(_) => assert!(false, "Data mistakenly accepted"),
            Err(e) => {
                let mut handled = false;
                if let Error::Yaml(ye) = &e {
                    handled = ye.description()
                        .starts_with("unknown field `bad_field`");
                };
                if !handled {
                    println!("Unexpected error {:?}", e);
                    assert!(false)
                }
            }
        }
    }
    
    #[test]
    fn test_layout_punctuation() {
        let out = Layout::from_file(PathBuf::from("tests/layout_key1.yaml"))
            .unwrap()
            .build(PanicWarn).0
            .unwrap();
        assert_eq!(
            out.views["base"]
                .rows[0]
                .buttons[0]
                .label,
            ::layout::Label::Text(CString::new("test").unwrap())
        );
    }

    #[test]
    fn test_layout_unicode() {
        let out = Layout::from_file(PathBuf::from("tests/layout_key2.yaml"))
            .unwrap()
            .build(PanicWarn).0
            .unwrap();
        assert_eq!(
            out.views["base"]
                .rows[0]
                .buttons[0]
                .label,
            ::layout::Label::Text(CString::new("test").unwrap())
        );
    }

    /// Test multiple codepoints
    #[test]
    fn test_layout_unicode_multi() {
        let out = Layout::from_file(PathBuf::from("tests/layout_key3.yaml"))
            .unwrap()
            .build(PanicWarn).0
            .unwrap();
        assert_eq!(
            out.views["base"]
                .rows[0]
                .buttons[0]
                .state.borrow()
                .keycodes.len(),
            2
        );
    }

    #[test]
    fn parsing_fallback() {
        assert!(Layout::from_resource(FALLBACK_LAYOUT_NAME)
            .map(|layout| layout.build(PanicWarn).0.unwrap())
            .is_ok()
        );
    }
    
    /// First fallback should be to builtin, not to FALLBACK_LAYOUT_NAME
    #[test]
    fn fallbacks_order() {
        let sources = list_layout_sources("nb", ArrangementKind::Base, None);
        
        assert_eq!(
            sources,
            vec!(
                (ArrangementKind::Base, DataSource::Resource("nb".into())),
                (
                    ArrangementKind::Base,
                    DataSource::Resource(FALLBACK_LAYOUT_NAME.into())
                ),
            )
        );
    }
    
    #[test]
    fn unicode_keysym() {
        let keysym = xkb::keysym_from_name(
            format!("U{:X}", "å".chars().next().unwrap() as u32).as_str(),
            xkb::KEYSYM_NO_FLAGS,
        );
        let keysym = xkb::keysym_to_utf8(keysym);
        assert_eq!(keysym, "å\0");
    }
    
    #[test]
    fn test_key_unicode() {
        assert_eq!(
            create_action(
                &hashmap!{
                    ".".into() => ButtonMeta {
                        icon: None,
                        keysym: None,
                        text: None,
                        action: None,
                        label: Some("test".into()),
                        outline: None,
                    }
                },
                ".",
                Vec::new(),
                &mut PanicWarn,
            ),
            ::action::Action::Submit {
                text: Some(CString::new(".").unwrap()),
                keys: vec!(::action::KeySym("U002E".into())),
            },
        );
    }
}
