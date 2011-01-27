/* 
 * Copyright (C) 2010-2011 Daiki Ueno <ueno@unixuser.org>
 * Copyright (C) 2010-2011 Red Hat, Inc.
 * 
 * This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public License
 * as published by the Free Software Foundation; either version 2 of
 * the License, or (at your option) any later version.
 * 
 * This library is distributed in the hope that it will be useful, but
 * WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * Lesser General Public License for more details.
 * 
 * You should have received a copy of the GNU Lesser General Public
 * License along with this library; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA
 * 02110-1301 USA
 */
#ifndef EEK_TYPES_H
#define EEK_TYPES_H 1

#include <glib-object.h>

G_BEGIN_DECLS

#define EEK_TYPE_KEYSYM_MATRIX (eek_keysym_matrix_get_type ())
#define EEK_TYPE_POINT (eek_point_get_type ())
#define EEK_TYPE_BOUNDS (eek_bounds_get_type ())
#define EEK_TYPE_OUTLINE (eek_outline_get_type ())
#define EEK_TYPE_COLOR (eek_color_get_type ())


/**
 * EekOrientation:
 * @EEK_ORIENTATION_VERTICAL: the elements will be arranged vertically
 * @EEK_ORIENTATION_HORIZONTAL: the elements will be arranged horizontally
 * @EEK_ORIENTATION_INVALID: used for error reporting
 *
 * Orientation of rows in sections.  Elements in a row will be
 * arranged with the specified orientation.
 */
typedef enum {
    EEK_ORIENTATION_VERTICAL,
    EEK_ORIENTATION_HORIZONTAL,
    EEK_ORIENTATION_INVALID = -1
} EekOrientation;

/**
 * EekModifierBehavior:
 * @EEK_MODIFIER_BEHAVIOR_NONE: do nothing when a modifier key is pressed
 * @EEK_MODIFIER_BEHAVIOR_LOCK: toggle the modifier status each time a
 * modifier key are pressed
 * @EEK_MODIFIER_BEHAVIOR_LATCH: enable the modifier when a modifier
 * key is pressed and keep it enabled until any key is pressed.
 *
 * Modifier handling mode.
 */
typedef enum {
    EEK_MODIFIER_BEHAVIOR_NONE,
    EEK_MODIFIER_BEHAVIOR_LOCK,
    EEK_MODIFIER_BEHAVIOR_LATCH
} EekModifierBehavior;
    
typedef struct _EekElement EekElement;
typedef struct _EekContainer EekContainer;
typedef struct _EekKey EekKey;
typedef struct _EekSection EekSection;
typedef struct _EekKeyboard EekKeyboard;

typedef struct _EekKeysymMatrix EekKeysymMatrix;
typedef struct _EekPoint EekPoint;
typedef struct _EekBounds EekBounds;
typedef struct _EekOutline EekOutline;
typedef struct _EekColor EekColor;

/**
 * EekKeysymMatrix:
 * @data: array of keysyms
 * @num_groups: the number of groups (rows)
 * @num_levels: the number of levels (columns)
 *
 * Symbol matrix of a key.
 */
struct _EekKeysymMatrix
{
    guint *data;
    gint num_groups;
    gint num_levels;
};

GType eek_keysym_matrix_get_type (void) G_GNUC_CONST;

/**
 * EekPoint:
 * @x: X coordinate of the point
 * @y: Y coordinate of the point
 *
 * 2D vertex
 */
struct _EekPoint
{
    gdouble x;
    gdouble y;
};

GType eek_point_get_type (void) G_GNUC_CONST;
void  eek_point_rotate   (EekPoint *point,
                          gint      angle);

/**
 * EekBounds:
 * @x: X coordinate of the top left point
 * @y: Y coordinate of the top left point
 * @width: width of the box
 * @height: height of the box
 *
 * The rectangle containing an element's bounding box.
 */
struct _EekBounds
{
    /*< public >*/
    gdouble x;
    gdouble y;
    gdouble width;
    gdouble height;
};

GType eek_bounds_get_type (void) G_GNUC_CONST;

G_INLINE_FUNC gdouble
eek_bounds_long_side (EekBounds *bounds)
{
    return bounds->width > bounds->height ? bounds->width : bounds->height;
}

/**
 * EekOutline:
 * @corner_radius: radius of corners of rounded polygon
 * @points: an array of points represents a polygon
 * @num_points: the length of @points
 *
 * 2D rounded polygon used to draw key shape
 */
struct _EekOutline
{
    gdouble corner_radius;
    EekPoint *points;
    gint num_points;
};

GType eek_outline_get_type (void) G_GNUC_CONST;

/**
 * EekColor:
 * @red: red component of color, between 0.0 and 1.0
 * @green: green component of color, between 0.0 and 1.0
 * @blue: blue component of color, between 0.0 and 1.0
 * @alpha: alpha component of color, between 0.0 and 1.0
 *
 * Color used for drawing.
 */
struct _EekColor
{
    gdouble red;
    gdouble green;
    gdouble blue;
    gdouble alpha;
};

GType     eek_color_get_type (void) G_GNUC_CONST;

EekColor *eek_color_new      (gdouble red,
                              gdouble green,
                              gdouble blue,
                              gdouble alpha);

G_END_DECLS
#endif  /* EEK_TYPES_H */
