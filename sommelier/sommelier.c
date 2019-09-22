// Copyright 2017 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "sommelier.h"

#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <gbm.h>
#include <libgen.h>
#include <linux/virtwl.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xcb/composite.h>
#include <xcb/xfixes.h>

#include "aura-shell-client-protocol.h"
#include "drm-server-protocol.h"
#include "keyboard-extension-unstable-v1-client-protocol.h"
#include "linux-dmabuf-unstable-v1-client-protocol.h"
#include "pointer-constraints-unstable-v1-client-protocol.h"
#include "relative-pointer-unstable-v1-client-protocol.h"
#include "text-input-unstable-v1-client-protocol.h"
#include "viewporter-client-protocol.h"
#include "xdg-shell-unstable-v6-client-protocol.h"

// Check that required macro definitions exist.
#ifndef XWAYLAND_PATH
#error XWAYLAND_PATH must be defined
#endif
#ifndef XWAYLAND_GL_DRIVER_PATH
#error XWAYLAND_GL_DRIVER_PATH must be defined
#endif
#ifndef XWAYLAND_SHM_DRIVER
#error XWAYLAND_SHM_DRIVER must be defined
#endif
#ifndef SHM_DRIVER
#error SHM_DRIVER must be defined
#endif
#ifndef VIRTWL_DEVICE
#error VIRTWL_DEVICE must be defined
#endif
#ifndef PEER_CMD_PREFIX
#error PEER_CMD_PREFIX must be defined
#endif
#ifndef FRAME_COLOR
#error FRAME_COLOR must be defined
#endif
#ifndef DARK_FRAME_COLOR
#error DARK_FRAME_COLOR must be defined
#endif

struct sl_data_source {
  struct sl_context* ctx;
  struct wl_data_source* internal;
};

enum {
  PROPERTY_WM_NAME,
  PROPERTY_WM_CLASS,
  PROPERTY_WM_TRANSIENT_FOR,
  PROPERTY_WM_NORMAL_HINTS,
  PROPERTY_WM_CLIENT_LEADER,
  PROPERTY_MOTIF_WM_HINTS,
  PROPERTY_NET_STARTUP_ID,
  PROPERTY_NET_WM_STATE,
  PROPERTY_GTK_THEME_VARIANT,
};

#define US_POSITION (1L << 0)
#define US_SIZE (1L << 1)
#define P_POSITION (1L << 2)
#define P_SIZE (1L << 3)
#define P_MIN_SIZE (1L << 4)
#define P_MAX_SIZE (1L << 5)
#define P_RESIZE_INC (1L << 6)
#define P_ASPECT (1L << 7)
#define P_BASE_SIZE (1L << 8)
#define P_WIN_GRAVITY (1L << 9)

struct sl_wm_size_hints {
  uint32_t flags;
  int32_t x, y;
  int32_t width, height;
  int32_t min_width, min_height;
  int32_t max_width, max_height;
  int32_t width_inc, height_inc;
  struct {
    int32_t x;
    int32_t y;
  } min_aspect, max_aspect;
  int32_t base_width, base_height;
  int32_t win_gravity;
};

#define MWM_HINTS_FUNCTIONS (1L << 0)
#define MWM_HINTS_DECORATIONS (1L << 1)
#define MWM_HINTS_INPUT_MODE (1L << 2)
#define MWM_HINTS_STATUS (1L << 3)

#define MWM_DECOR_ALL (1L << 0)
#define MWM_DECOR_BORDER (1L << 1)
#define MWM_DECOR_RESIZEH (1L << 2)
#define MWM_DECOR_TITLE (1L << 3)
#define MWM_DECOR_MENU (1L << 4)
#define MWM_DECOR_MINIMIZE (1L << 5)
#define MWM_DECOR_MAXIMIZE (1L << 6)

struct sl_mwm_hints {
  uint32_t flags;
  uint32_t functions;
  uint32_t decorations;
  int32_t input_mode;
  uint32_t status;
};

#define NET_WM_MOVERESIZE_SIZE_TOPLEFT 0
#define NET_WM_MOVERESIZE_SIZE_TOP 1
#define NET_WM_MOVERESIZE_SIZE_TOPRIGHT 2
#define NET_WM_MOVERESIZE_SIZE_RIGHT 3
#define NET_WM_MOVERESIZE_SIZE_BOTTOMRIGHT 4
#define NET_WM_MOVERESIZE_SIZE_BOTTOM 5
#define NET_WM_MOVERESIZE_SIZE_BOTTOMLEFT 6
#define NET_WM_MOVERESIZE_SIZE_LEFT 7
#define NET_WM_MOVERESIZE_MOVE 8

#define NET_WM_STATE_REMOVE 0
#define NET_WM_STATE_ADD 1
#define NET_WM_STATE_TOGGLE 2

#define WM_STATE_WITHDRAWN 0
#define WM_STATE_NORMAL 1
#define WM_STATE_ICONIC 3

#define SEND_EVENT_MASK 0x80

#define MIN_SCALE 0.1
#define MAX_SCALE 10.0

#define MIN_DPI 72
#define MAX_DPI 9600

#define XCURSOR_SIZE_BASE 24

#ifndef UNIX_PATH_MAX
#define UNIX_PATH_MAX 108
#endif

#define LOCK_SUFFIX ".lock"
#define LOCK_SUFFIXLEN 5

#define APPLICATION_ID_FORMAT_PREFIX "org.chromium.termina"
#define XID_APPLICATION_ID_FORMAT APPLICATION_ID_FORMAT_PREFIX ".xid.%d"
#define WM_CLIENT_LEADER_APPLICATION_ID_FORMAT \
  APPLICATION_ID_FORMAT_PREFIX ".wmclientleader.%d"
#define WM_CLASS_APPLICATION_ID_FORMAT \
  APPLICATION_ID_FORMAT_PREFIX ".wmclass.%s"

#define MIN_AURA_SHELL_VERSION 6

// Performs an asprintf operation and checks the result for validity and calls
// abort() if there's a failure. Returns a newly allocated string rather than
// taking a double pointer argument like asprintf.
__attribute__((__format__(__printf__, 1, 0))) static char* sl_xasprintf(
    const char* fmt, ...) {
  char* str;
  va_list args;
  va_start(args, fmt);
  int rv = vasprintf(&str, fmt, args);
  assert(rv >= 0);
  UNUSED(rv);
  va_end(args);
  return str;
}

struct sl_mmap* sl_mmap_create(int fd,
                               size_t size,
                               size_t bpp,
                               size_t num_planes,
                               size_t offset0,
                               size_t stride0,
                               size_t offset1,
                               size_t stride1,
                               size_t y_ss0,
                               size_t y_ss1) {
  struct sl_mmap* map;

  map = malloc(sizeof(*map));
  map->refcount = 1;
  map->fd = fd;
  map->size = size;
  map->num_planes = num_planes;
  map->bpp = bpp;
  map->offset[0] = offset0;
  map->stride[0] = stride0;
  map->offset[1] = offset1;
  map->stride[1] = stride1;
  map->y_ss[0] = y_ss0;
  map->y_ss[1] = y_ss1;
  map->begin_write = NULL;
  map->end_write = NULL;
  map->buffer_resource = NULL;
  map->addr =
      mmap(NULL, size + offset0, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  assert(map->addr != MAP_FAILED);

  return map;
}

struct sl_mmap* sl_mmap_ref(struct sl_mmap* map) {
  map->refcount++;
  return map;
}

void sl_mmap_unref(struct sl_mmap* map) {
  if (map->refcount-- == 1) {
    munmap(map->addr, map->size + map->offset[0]);
    close(map->fd);
    free(map);
  }
}

struct sl_sync_point* sl_sync_point_create(int fd) {
  struct sl_sync_point* sync_point;

  sync_point = malloc(sizeof(*sync_point));
  sync_point->fd = fd;
  sync_point->sync = NULL;

  return sync_point;
}

void sl_sync_point_destroy(struct sl_sync_point* sync_point) {
  close(sync_point->fd);
  free(sync_point);
}

static void sl_internal_xdg_shell_ping(void* data,
                                       struct zxdg_shell_v6* xdg_shell,
                                       uint32_t serial) {
  zxdg_shell_v6_pong(xdg_shell, serial);
}

static const struct zxdg_shell_v6_listener sl_internal_xdg_shell_listener = {
    sl_internal_xdg_shell_ping};

static void sl_send_configure_notify(struct sl_window* window) {
  xcb_configure_notify_event_t event = {
      .response_type = XCB_CONFIGURE_NOTIFY,
      .event = window->id,
      .window = window->id,
      .above_sibling = XCB_WINDOW_NONE,
      .x = window->x,
      .y = window->y,
      .width = window->width,
      .height = window->height,
      .border_width = window->border_width,
      .override_redirect = 0,
      .pad0 = 0,
      .pad1 = 0,
  };

  xcb_send_event(window->ctx->connection, 0, window->id,
                 XCB_EVENT_MASK_STRUCTURE_NOTIFY, (char*)&event);
}

static void sl_adjust_window_size_for_screen_size(struct sl_window* window) {
  struct sl_context* ctx = window->ctx;

  // Clamp size to screen.
  window->width = MIN(window->width, ctx->screen->width_in_pixels);
  window->height = MIN(window->height, ctx->screen->height_in_pixels);
}

static void sl_adjust_window_position_for_screen_size(
    struct sl_window* window) {
  struct sl_context* ctx = window->ctx;

  // Center horizontally/vertically.
  window->x = ctx->screen->width_in_pixels / 2 - window->width / 2;
  window->y = ctx->screen->height_in_pixels / 2 - window->height / 2;
}

static void sl_configure_window(struct sl_window* window) {
  assert(!window->pending_config.serial);

  if (window->next_config.mask) {
    int values[5];
    int x = window->x;
    int y = window->y;
    int i = 0;

    xcb_configure_window(window->ctx->connection, window->frame_id,
                         window->next_config.mask, window->next_config.values);

    if (window->next_config.mask & XCB_CONFIG_WINDOW_X)
      x = window->next_config.values[i++];
    if (window->next_config.mask & XCB_CONFIG_WINDOW_Y)
      y = window->next_config.values[i++];
    if (window->next_config.mask & XCB_CONFIG_WINDOW_WIDTH)
      window->width = window->next_config.values[i++];
    if (window->next_config.mask & XCB_CONFIG_WINDOW_HEIGHT)
      window->height = window->next_config.values[i++];
    if (window->next_config.mask & XCB_CONFIG_WINDOW_BORDER_WIDTH)
      window->border_width = window->next_config.values[i++];

    // Set x/y to origin in case window gravity is not northwest as expected.
    assert(window->managed);
    values[0] = 0;
    values[1] = 0;
    values[2] = window->width;
    values[3] = window->height;
    values[4] = window->border_width;
    xcb_configure_window(
        window->ctx->connection, window->id,
        XCB_CONFIG_WINDOW_X | XCB_CONFIG_WINDOW_Y | XCB_CONFIG_WINDOW_WIDTH |
            XCB_CONFIG_WINDOW_HEIGHT | XCB_CONFIG_WINDOW_BORDER_WIDTH,
        values);

    if (x != window->x || y != window->y) {
      window->x = x;
      window->y = y;
      sl_send_configure_notify(window);
    }
  }

  if (window->managed) {
    xcb_change_property(window->ctx->connection, XCB_PROP_MODE_REPLACE,
                        window->id, window->ctx->atoms[ATOM_NET_WM_STATE].value,
                        XCB_ATOM_ATOM, 32, window->next_config.states_length,
                        window->next_config.states);
  }

  window->pending_config = window->next_config;
  window->next_config.serial = 0;
  window->next_config.mask = 0;
  window->next_config.states_length = 0;
}

static void sl_set_input_focus(struct sl_context* ctx,
                               struct sl_window* window) {
  if (window) {
    xcb_client_message_event_t event = {
        .response_type = XCB_CLIENT_MESSAGE,
        .format = 32,
        .window = window->id,
        .type = ctx->atoms[ATOM_WM_PROTOCOLS].value,
        .data.data32 =
            {
                ctx->atoms[ATOM_WM_TAKE_FOCUS].value,
                XCB_CURRENT_TIME,
            },
    };

    if (!window->managed)
      return;

    xcb_send_event(ctx->connection, 0, window->id,
                   XCB_EVENT_MASK_SUBSTRUCTURE_REDIRECT, (char*)&event);

    xcb_set_input_focus(ctx->connection, XCB_INPUT_FOCUS_NONE, window->id,
                        XCB_CURRENT_TIME);
  } else {
    xcb_set_input_focus(ctx->connection, XCB_INPUT_FOCUS_NONE, XCB_NONE,
                        XCB_CURRENT_TIME);
  }
}

void sl_restack_windows(struct sl_context* ctx, uint32_t focus_resource_id) {
  struct sl_window* sibling;
  uint32_t values[1];

  wl_list_for_each(sibling, &ctx->windows, link) {
    if (!sibling->managed)
      continue;

    // Move focus window to the top and all other windows to the bottom.
    values[0] = sibling->host_surface_id == focus_resource_id
                    ? XCB_STACK_MODE_ABOVE
                    : XCB_STACK_MODE_BELOW;
    xcb_configure_window(ctx->connection, sibling->frame_id,
                         XCB_CONFIG_WINDOW_STACK_MODE, values);
  }
}

void sl_roundtrip(struct sl_context* ctx) {
  free(xcb_get_input_focus_reply(ctx->connection,
                                 xcb_get_input_focus(ctx->connection), NULL));
}

int sl_process_pending_configure_acks(struct sl_window* window,
                                      struct sl_host_surface* host_surface) {
  if (!window->pending_config.serial)
    return 0;

  if (window->managed && host_surface) {
    int width = window->width + window->border_width * 2;
    int height = window->height + window->border_width * 2;
    // Early out if we expect contents to match window size at some point in
    // the future.
    if (width != host_surface->contents_width ||
        height != host_surface->contents_height) {
      return 0;
    }
  }

  if (window->xdg_surface) {
    zxdg_surface_v6_ack_configure(window->xdg_surface,
                                  window->pending_config.serial);
  }
  window->pending_config.serial = 0;

  if (window->next_config.serial)
    sl_configure_window(window);

  return 1;
}

static void sl_internal_xdg_surface_configure(
    void* data, struct zxdg_surface_v6* xdg_surface, uint32_t serial) {
  struct sl_window* window = zxdg_surface_v6_get_user_data(xdg_surface);

  window->next_config.serial = serial;
  if (!window->pending_config.serial) {
    struct wl_resource* host_resource;
    struct sl_host_surface* host_surface = NULL;

    host_resource =
        wl_client_get_object(window->ctx->client, window->host_surface_id);
    if (host_resource)
      host_surface = wl_resource_get_user_data(host_resource);

    sl_configure_window(window);

    if (sl_process_pending_configure_acks(window, host_surface)) {
      if (host_surface)
        wl_surface_commit(host_surface->proxy);
    }
  }
}

static const struct zxdg_surface_v6_listener sl_internal_xdg_surface_listener =
    {sl_internal_xdg_surface_configure};

static void sl_internal_xdg_toplevel_configure(
    void* data,
    struct zxdg_toplevel_v6* xdg_toplevel,
    int32_t width,
    int32_t height,
    struct wl_array* states) {
  struct sl_window* window = zxdg_toplevel_v6_get_user_data(xdg_toplevel);
  int activated = 0;
  uint32_t* state;
  int i = 0;

  if (!window->managed)
    return;

  if (width && height) {
    int32_t width_in_pixels = width * window->ctx->scale;
    int32_t height_in_pixels = height * window->ctx->scale;
    int i = 0;

    window->next_config.mask = XCB_CONFIG_WINDOW_WIDTH |
                               XCB_CONFIG_WINDOW_HEIGHT |
                               XCB_CONFIG_WINDOW_BORDER_WIDTH;
    if (!(window->size_flags & (US_POSITION | P_POSITION))) {
      window->next_config.mask |= XCB_CONFIG_WINDOW_X | XCB_CONFIG_WINDOW_Y;
      window->next_config.values[i++] =
          window->ctx->screen->width_in_pixels / 2 - width_in_pixels / 2;
      window->next_config.values[i++] =
          window->ctx->screen->height_in_pixels / 2 - height_in_pixels / 2;
    }
    window->next_config.values[i++] = width_in_pixels;
    window->next_config.values[i++] = height_in_pixels;
    window->next_config.values[i++] = 0;
  }

  window->allow_resize = 1;
  wl_array_for_each(state, states) {
    if (*state == ZXDG_TOPLEVEL_V6_STATE_FULLSCREEN) {
      window->allow_resize = 0;
      window->next_config.states[i++] =
          window->ctx->atoms[ATOM_NET_WM_STATE_FULLSCREEN].value;
    }
    if (*state == ZXDG_TOPLEVEL_V6_STATE_MAXIMIZED) {
      window->allow_resize = 0;
      window->next_config.states[i++] =
          window->ctx->atoms[ATOM_NET_WM_STATE_MAXIMIZED_VERT].value;
      window->next_config.states[i++] =
          window->ctx->atoms[ATOM_NET_WM_STATE_MAXIMIZED_HORZ].value;
    }
    if (*state == ZXDG_TOPLEVEL_V6_STATE_ACTIVATED)
      activated = 1;
    if (*state == ZXDG_TOPLEVEL_V6_STATE_RESIZING)
      window->allow_resize = 0;
  }

  if (activated != window->activated) {
    if (activated != (window->ctx->host_focus_window == window)) {
      window->ctx->host_focus_window = activated ? window : NULL;
      window->ctx->needs_set_input_focus = 1;
    }
    window->activated = activated;
  }

  window->next_config.states_length = i;
}

static void sl_internal_xdg_toplevel_close(
    void* data, struct zxdg_toplevel_v6* xdg_toplevel) {
  struct sl_window* window = zxdg_toplevel_v6_get_user_data(xdg_toplevel);
  xcb_client_message_event_t event = {
      .response_type = XCB_CLIENT_MESSAGE,
      .format = 32,
      .window = window->id,
      .type = window->ctx->atoms[ATOM_WM_PROTOCOLS].value,
      .data.data32 =
          {
              window->ctx->atoms[ATOM_WM_DELETE_WINDOW].value,
              XCB_CURRENT_TIME,
          },
  };

  xcb_send_event(window->ctx->connection, 0, window->id,
                 XCB_EVENT_MASK_NO_EVENT, (const char*)&event);
}

static const struct zxdg_toplevel_v6_listener
    sl_internal_xdg_toplevel_listener = {sl_internal_xdg_toplevel_configure,
                                         sl_internal_xdg_toplevel_close};

static void sl_internal_xdg_popup_configure(void* data,
                                            struct zxdg_popup_v6* xdg_popup,
                                            int32_t x,
                                            int32_t y,
                                            int32_t width,
                                            int32_t height) {}

static void sl_internal_xdg_popup_done(void* data,
                                       struct zxdg_popup_v6* zxdg_popup_v6) {}

static const struct zxdg_popup_v6_listener sl_internal_xdg_popup_listener = {
    sl_internal_xdg_popup_configure, sl_internal_xdg_popup_done};

static void sl_window_set_wm_state(struct sl_window* window, int state) {
  struct sl_context* ctx = window->ctx;
  uint32_t values[2];

  values[0] = state;
  values[1] = XCB_WINDOW_NONE;

  xcb_change_property(ctx->connection, XCB_PROP_MODE_REPLACE, window->id,
                      ctx->atoms[ATOM_WM_STATE].value,
                      ctx->atoms[ATOM_WM_STATE].value, 32, 2, values);
}

void sl_window_update(struct sl_window* window) {
  struct wl_resource* host_resource = NULL;
  struct sl_host_surface* host_surface;
  struct sl_context* ctx = window->ctx;
  struct sl_window* parent = NULL;

  if (window->host_surface_id) {
    host_resource = wl_client_get_object(ctx->client, window->host_surface_id);
    if (host_resource && window->unpaired) {
      wl_list_remove(&window->link);
      wl_list_insert(&ctx->windows, &window->link);
      window->unpaired = 0;
    }
  } else if (!window->unpaired) {
    wl_list_remove(&window->link);
    wl_list_insert(&ctx->unpaired_windows, &window->link);
    window->unpaired = 1;
  }

  if (!host_resource) {
    if (window->aura_surface) {
      zaura_surface_destroy(window->aura_surface);
      window->aura_surface = NULL;
    }
    if (window->xdg_toplevel) {
      zxdg_toplevel_v6_destroy(window->xdg_toplevel);
      window->xdg_toplevel = NULL;
    }
    if (window->xdg_popup) {
      zxdg_popup_v6_destroy(window->xdg_popup);
      window->xdg_popup = NULL;
    }
    if (window->xdg_surface) {
      zxdg_surface_v6_destroy(window->xdg_surface);
      window->xdg_surface = NULL;
    }
    window->realized = 0;
    return;
  }

  host_surface = wl_resource_get_user_data(host_resource);
  assert(host_surface);
  assert(!host_surface->has_role);

  assert(ctx->xdg_shell);
  assert(ctx->xdg_shell->internal);

  if (window->managed) {
    if (window->transient_for != XCB_WINDOW_NONE) {
      struct sl_window* sibling;

      wl_list_for_each(sibling, &ctx->windows, link) {
        if (sibling->id == window->transient_for) {
          if (sibling->xdg_toplevel)
            parent = sibling;
          break;
        }
      }
    }
  }

  // If we have a transient parent, but could not find it in the list of
  // realized windows, then pick the window that had the last event for the
  // parent.  We update this again when we gain focus, so if we picked the wrong
  // one it can get corrected at that point (but it's also possible the parent
  // will never be realized, which is why selecting one here is important).
  if (!window->managed ||
      (!parent && window->transient_for != XCB_WINDOW_NONE)) {
    struct sl_window* sibling;
    uint32_t parent_last_event_serial = 0;

    wl_list_for_each(sibling, &ctx->windows, link) {
      struct wl_resource* sibling_host_resource;
      struct sl_host_surface* sibling_host_surface;

      if (!sibling->realized)
        continue;

      sibling_host_resource =
          wl_client_get_object(ctx->client, sibling->host_surface_id);
      if (!sibling_host_resource)
        continue;

      // Any parent will do but prefer last event window.
      sibling_host_surface = wl_resource_get_user_data(sibling_host_resource);
      if (parent_last_event_serial > sibling_host_surface->last_event_serial)
        continue;

      // Do not use ourselves as the parent.
      if (sibling->host_surface_id == window->host_surface_id)
        continue;

      parent = sibling;
      parent_last_event_serial = sibling_host_surface->last_event_serial;
    }
  }

  if (!window->depth) {
    xcb_get_geometry_reply_t* geometry_reply = xcb_get_geometry_reply(
        ctx->connection, xcb_get_geometry(ctx->connection, window->id), NULL);
    if (geometry_reply) {
      window->depth = geometry_reply->depth;
      free(geometry_reply);
    }
  }

  if (!window->xdg_surface) {
    window->xdg_surface = zxdg_shell_v6_get_xdg_surface(
        ctx->xdg_shell->internal, host_surface->proxy);
    zxdg_surface_v6_set_user_data(window->xdg_surface, window);
    zxdg_surface_v6_add_listener(window->xdg_surface,
                                 &sl_internal_xdg_surface_listener, window);
  }

  if (ctx->aura_shell) {
    uint32_t frame_color;

    if (!window->aura_surface) {
      window->aura_surface = zaura_shell_get_aura_surface(
          ctx->aura_shell->internal, host_surface->proxy);
    }

    zaura_surface_set_frame(window->aura_surface,
                            window->decorated
                                ? ZAURA_SURFACE_FRAME_TYPE_NORMAL
                                : window->depth == 32
                                      ? ZAURA_SURFACE_FRAME_TYPE_NONE
                                      : ZAURA_SURFACE_FRAME_TYPE_SHADOW);

    frame_color = window->dark_frame ? ctx->dark_frame_color : ctx->frame_color;
    zaura_surface_set_frame_colors(window->aura_surface, frame_color,
                                   frame_color);
    zaura_surface_set_startup_id(window->aura_surface, window->startup_id);

    if (ctx->application_id) {
      zaura_surface_set_application_id(window->aura_surface,
                                       ctx->application_id);
    } else {
      // Don't set application id for X11 override redirect. This prevents
      // aura shell from thinking that these are regular application windows
      // that should appear in application lists.
      if (!ctx->xwayland || window->managed) {
        char* application_id_str;
        if (window->clazz) {
          application_id_str =
              sl_xasprintf(WM_CLASS_APPLICATION_ID_FORMAT, window->clazz);
        } else if (window->client_leader != XCB_WINDOW_NONE) {
          application_id_str = sl_xasprintf(
              WM_CLIENT_LEADER_APPLICATION_ID_FORMAT, window->client_leader);
        } else {
          application_id_str =
              sl_xasprintf(XID_APPLICATION_ID_FORMAT, window->id);
        }

        zaura_surface_set_application_id(window->aura_surface,
                                         application_id_str);
        free(application_id_str);
      }
    }
  }

  // Always use top-level surface for X11 windows as we can't control when the
  // window is closed.
  if (ctx->xwayland || !parent) {
    if (!window->xdg_toplevel) {
      window->xdg_toplevel = zxdg_surface_v6_get_toplevel(window->xdg_surface);
      zxdg_toplevel_v6_set_user_data(window->xdg_toplevel, window);
      zxdg_toplevel_v6_add_listener(window->xdg_toplevel,
                                    &sl_internal_xdg_toplevel_listener, window);
    }
    if (parent)
      zxdg_toplevel_v6_set_parent(window->xdg_toplevel, parent->xdg_toplevel);
    if (window->name)
      zxdg_toplevel_v6_set_title(window->xdg_toplevel, window->name);
    if (window->size_flags & P_MIN_SIZE) {
      zxdg_toplevel_v6_set_min_size(window->xdg_toplevel,
                                    window->min_width / ctx->scale,
                                    window->min_height / ctx->scale);
    }
    if (window->size_flags & P_MAX_SIZE) {
      zxdg_toplevel_v6_set_max_size(window->xdg_toplevel,
                                    window->max_width / ctx->scale,
                                    window->max_height / ctx->scale);
    }
    if (window->maximized) {
      zxdg_toplevel_v6_set_maximized(window->xdg_toplevel);
    }
  } else if (!window->xdg_popup) {
    struct zxdg_positioner_v6* positioner;

    positioner = zxdg_shell_v6_create_positioner(ctx->xdg_shell->internal);
    assert(positioner);
    zxdg_positioner_v6_set_anchor(
        positioner,
        ZXDG_POSITIONER_V6_ANCHOR_TOP | ZXDG_POSITIONER_V6_ANCHOR_LEFT);
    zxdg_positioner_v6_set_gravity(
        positioner,
        ZXDG_POSITIONER_V6_GRAVITY_BOTTOM | ZXDG_POSITIONER_V6_GRAVITY_RIGHT);
    zxdg_positioner_v6_set_anchor_rect(
        positioner, (window->x - parent->x) / ctx->scale,
        (window->y - parent->y) / ctx->scale, 1, 1);

    window->xdg_popup = zxdg_surface_v6_get_popup(
        window->xdg_surface, parent->xdg_surface, positioner);
    zxdg_popup_v6_set_user_data(window->xdg_popup, window);
    zxdg_popup_v6_add_listener(window->xdg_popup,
                               &sl_internal_xdg_popup_listener, window);

    zxdg_positioner_v6_destroy(positioner);
  }

  if ((window->size_flags & (US_POSITION | P_POSITION)) && parent &&
      ctx->aura_shell) {
    zaura_surface_set_parent(window->aura_surface, parent->aura_surface,
                             (window->x - parent->x) / ctx->scale,
                             (window->y - parent->y) / ctx->scale);
  }

  wl_surface_commit(host_surface->proxy);
  if (host_surface->contents_width && host_surface->contents_height)
    window->realized = 1;
}

static void sl_host_buffer_destroy(struct wl_client* client,
                                   struct wl_resource* resource) {
  wl_resource_destroy(resource);
}

static const struct wl_buffer_interface sl_buffer_implementation = {
    sl_host_buffer_destroy};

static void sl_buffer_release(void* data, struct wl_buffer* buffer) {
  struct sl_host_buffer* host = wl_buffer_get_user_data(buffer);

  wl_buffer_send_release(host->resource);
}

static const struct wl_buffer_listener sl_buffer_listener = {sl_buffer_release};

static void sl_destroy_host_buffer(struct wl_resource* resource) {
  struct sl_host_buffer* host = wl_resource_get_user_data(resource);

  if (host->proxy)
    wl_buffer_destroy(host->proxy);
  if (host->shm_mmap) {
    host->shm_mmap->buffer_resource = NULL;
    sl_mmap_unref(host->shm_mmap);
  }
  if (host->sync_point) {
    sl_sync_point_destroy(host->sync_point);
  }
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

struct sl_host_buffer* sl_create_host_buffer(struct wl_client* client,
                                             uint32_t id,
                                             struct wl_buffer* proxy,
                                             int32_t width,
                                             int32_t height) {
  struct sl_host_buffer* host_buffer;

  host_buffer = malloc(sizeof(*host_buffer));
  assert(host_buffer);

  host_buffer->width = width;
  host_buffer->height = height;
  host_buffer->resource =
      wl_resource_create(client, &wl_buffer_interface, 1, id);
  wl_resource_set_implementation(host_buffer->resource,
                                 &sl_buffer_implementation, host_buffer,
                                 sl_destroy_host_buffer);
  host_buffer->shm_mmap = NULL;
  host_buffer->shm_format = 0;
  host_buffer->proxy = proxy;
  if (host_buffer->proxy) {
    wl_buffer_set_user_data(host_buffer->proxy, host_buffer);
    wl_buffer_add_listener(host_buffer->proxy, &sl_buffer_listener,
                           host_buffer);
  }
  host_buffer->sync_point = NULL;

  return host_buffer;
}

static void sl_internal_data_offer_destroy(struct sl_data_offer* host) {
  wl_data_offer_destroy(host->internal);
  wl_array_release(&host->atoms);
  wl_array_release(&host->cookies);
  free(host);
}

static void sl_set_selection(struct sl_context* ctx,
                             struct sl_data_offer* data_offer) {
  if (ctx->selection_data_offer) {
    sl_internal_data_offer_destroy(ctx->selection_data_offer);
    ctx->selection_data_offer = NULL;
  }

  if (ctx->clipboard_manager) {
    if (!data_offer) {
      if (ctx->selection_owner == ctx->selection_window)
        xcb_set_selection_owner(ctx->connection, XCB_ATOM_NONE,
                                ctx->atoms[ATOM_CLIPBOARD].value,
                                ctx->selection_timestamp);
      return;
    }

    int atoms = data_offer->cookies.size / sizeof(xcb_intern_atom_cookie_t);
    wl_array_add(&data_offer->atoms, sizeof(xcb_atom_t) * (atoms + 2));
    ((xcb_atom_t*)data_offer->atoms.data)[0] = ctx->atoms[ATOM_TARGETS].value;
    ((xcb_atom_t*)data_offer->atoms.data)[1] = ctx->atoms[ATOM_TIMESTAMP].value;
    for (int i = 0; i < atoms; i++) {
      xcb_intern_atom_cookie_t cookie =
          ((xcb_intern_atom_cookie_t*)data_offer->cookies.data)[i];
      xcb_intern_atom_reply_t* reply =
          xcb_intern_atom_reply(ctx->connection, cookie, NULL);
      if (reply) {
        ((xcb_atom_t*)data_offer->atoms.data)[i + 2] = reply->atom;
        free(reply);
      }
    }

    xcb_set_selection_owner(ctx->connection, ctx->selection_window,
                            ctx->atoms[ATOM_CLIPBOARD].value, XCB_CURRENT_TIME);
  }

  ctx->selection_data_offer = data_offer;
}

static void sl_internal_data_offer_offer(void* data,
                                         struct wl_data_offer* data_offer,
                                         const char* type) {
  struct sl_data_offer* host = data;
  xcb_intern_atom_cookie_t* cookie =
      wl_array_add(&host->cookies, sizeof(xcb_intern_atom_cookie_t));
  *cookie = xcb_intern_atom(host->ctx->connection, 0, strlen(type), type);
}

static void sl_internal_data_offer_source_actions(
    void* data, struct wl_data_offer* data_offer, uint32_t source_actions) {}

static void sl_internal_data_offer_action(void* data,
                                          struct wl_data_offer* data_offer,
                                          uint32_t dnd_action) {}

static const struct wl_data_offer_listener sl_internal_data_offer_listener = {
    sl_internal_data_offer_offer, sl_internal_data_offer_source_actions,
    sl_internal_data_offer_action};

static void sl_internal_data_device_data_offer(
    void* data,
    struct wl_data_device* data_device,
    struct wl_data_offer* data_offer) {
  struct sl_context* ctx = (struct sl_context*)data;
  struct sl_data_offer* host_data_offer;

  host_data_offer = malloc(sizeof(*host_data_offer));
  assert(host_data_offer);

  host_data_offer->ctx = ctx;
  host_data_offer->internal = data_offer;
  wl_array_init(&host_data_offer->atoms);
  wl_array_init(&host_data_offer->cookies);

  wl_data_offer_add_listener(host_data_offer->internal,
                             &sl_internal_data_offer_listener, host_data_offer);
}

static void sl_internal_data_device_enter(void* data,
                                          struct wl_data_device* data_device,
                                          uint32_t serial,
                                          struct wl_surface* surface,
                                          wl_fixed_t x,
                                          wl_fixed_t y,
                                          struct wl_data_offer* data_offer) {}

static void sl_internal_data_device_leave(void* data,
                                          struct wl_data_device* data_device) {}

static void sl_internal_data_device_motion(void* data,
                                           struct wl_data_device* data_device,
                                           uint32_t time,
                                           wl_fixed_t x,
                                           wl_fixed_t y) {}

static void sl_internal_data_device_drop(void* data,
                                         struct wl_data_device* data_device) {}

static void sl_internal_data_device_selection(
    void* data,
    struct wl_data_device* data_device,
    struct wl_data_offer* data_offer) {
  struct sl_context* ctx = (struct sl_context*)data;
  struct sl_data_offer* host_data_offer =
      data_offer ? wl_data_offer_get_user_data(data_offer) : NULL;

  sl_set_selection(ctx, host_data_offer);
}

static const struct wl_data_device_listener sl_internal_data_device_listener = {
    sl_internal_data_device_data_offer, sl_internal_data_device_enter,
    sl_internal_data_device_leave,      sl_internal_data_device_motion,
    sl_internal_data_device_drop,       sl_internal_data_device_selection};

void sl_host_seat_added(struct sl_host_seat* host) {
  struct sl_context* ctx = host->seat->ctx;

  if (ctx->default_seat)
    return;

  ctx->default_seat = host;

  // Get data device for selections.
  if (ctx->data_device_manager && ctx->data_device_manager->internal) {
    ctx->selection_data_device = wl_data_device_manager_get_data_device(
        ctx->data_device_manager->internal, host->proxy);
    wl_data_device_add_listener(ctx->selection_data_device,
                                &sl_internal_data_device_listener, ctx);
  }
}

void sl_host_seat_removed(struct sl_host_seat* host) {
  if (host->seat->ctx->default_seat == host)
    host->seat->ctx->default_seat = NULL;
}

struct sl_global* sl_global_create(struct sl_context* ctx,
                                   const struct wl_interface* interface,
                                   int version,
                                   void* data,
                                   wl_global_bind_func_t bind) {
  struct sl_host_registry* registry;
  struct sl_global* global;

  assert(version > 0);
  assert(version <= interface->version);

  global = malloc(sizeof *global);
  assert(global);

  global->ctx = ctx;
  global->name = ctx->next_global_id++;
  global->interface = interface;
  global->version = version;
  global->data = data;
  global->bind = bind;
  wl_list_insert(ctx->globals.prev, &global->link);

  wl_list_for_each(registry, &ctx->registries, link) {
    wl_resource_post_event(registry->resource, WL_REGISTRY_GLOBAL, global->name,
                           global->interface->name, global->version);
  }

  return global;
}

static void sl_global_destroy(struct sl_global* global) {
  struct sl_host_registry* registry;

  wl_list_for_each(registry, &global->ctx->registries, link)
      wl_resource_post_event(registry->resource, WL_REGISTRY_GLOBAL_REMOVE,
                             global->name);
  wl_list_remove(&global->link);
  free(global);
}

static void sl_registry_handler(void* data,
                                struct wl_registry* registry,
                                uint32_t id,
                                const char* interface,
                                uint32_t version) {
  struct sl_context* ctx = (struct sl_context*)data;

  if (strcmp(interface, "wl_compositor") == 0) {
    struct sl_compositor* compositor = malloc(sizeof(struct sl_compositor));
    assert(compositor);
    compositor->ctx = ctx;
    compositor->id = id;
    assert(version >= 3);
    compositor->version = 3;
    compositor->internal = wl_registry_bind(
        registry, id, &wl_compositor_interface, compositor->version);
    assert(!ctx->compositor);
    ctx->compositor = compositor;
    compositor->host_global = sl_compositor_global_create(ctx);
  } else if (strcmp(interface, "wl_subcompositor") == 0) {
    struct sl_subcompositor* subcompositor =
        malloc(sizeof(struct sl_subcompositor));
    assert(subcompositor);
    subcompositor->ctx = ctx;
    subcompositor->id = id;
    assert(!ctx->subcompositor);
    ctx->subcompositor = subcompositor;
    subcompositor->host_global = sl_subcompositor_global_create(ctx);
  } else if (strcmp(interface, "wl_shm") == 0) {
    struct sl_shm* shm = malloc(sizeof(struct sl_shm));
    assert(shm);
    shm->ctx = ctx;
    shm->id = id;
    shm->internal = wl_registry_bind(registry, id, &wl_shm_interface, 1);
    assert(!ctx->shm);
    ctx->shm = shm;
    shm->host_global = sl_shm_global_create(ctx);
  } else if (strcmp(interface, "wl_shell") == 0) {
    struct sl_shell* shell = malloc(sizeof(struct sl_shell));
    assert(shell);
    shell->ctx = ctx;
    shell->id = id;
    assert(!ctx->shell);
    ctx->shell = shell;
    shell->host_global = sl_shell_global_create(ctx);
  } else if (strcmp(interface, "wl_output") == 0) {
    struct sl_output* output = malloc(sizeof(struct sl_output));
    assert(output);
    output->ctx = ctx;
    output->id = id;
    output->version = MIN(3, version);
    output->host_global = sl_output_global_create(output);
    wl_list_insert(&ctx->outputs, &output->link);
  } else if (strcmp(interface, "wl_seat") == 0) {
    struct sl_seat* seat = malloc(sizeof(struct sl_seat));
    assert(seat);
    seat->ctx = ctx;
    seat->id = id;
    seat->version = MIN(5, version);
    seat->last_serial = 0;
    seat->host_global = sl_seat_global_create(seat);
    wl_list_insert(&ctx->seats, &seat->link);
  } else if (strcmp(interface, "zwp_relative_pointer_manager_v1") == 0) {
    struct sl_relative_pointer_manager* relative_pointer =
        malloc(sizeof(struct sl_relative_pointer_manager));
    assert(relative_pointer);
    relative_pointer->ctx = ctx;
    relative_pointer->id = id;
    relative_pointer->internal = wl_registry_bind(
        registry, id, &zwp_relative_pointer_manager_v1_interface, 1);
    assert(!ctx->relative_pointer_manager);
    ctx->relative_pointer_manager = relative_pointer;
    relative_pointer->host_global =
        sl_relative_pointer_manager_global_create(ctx);
  } else if (strcmp(interface, "zwp_pointer_constraints_v1") == 0) {
    struct sl_pointer_constraints* pointer_constraints =
        malloc(sizeof(struct sl_pointer_constraints));
    assert(pointer_constraints);
    pointer_constraints->ctx = ctx;
    pointer_constraints->id = id;
    pointer_constraints->internal = wl_registry_bind(
        registry, id, &zwp_pointer_constraints_v1_interface, 1);
    assert(!ctx->pointer_constraints);
    ctx->pointer_constraints = pointer_constraints;
    pointer_constraints->host_global =
        sl_pointer_constraints_global_create(ctx);
  } else if (strcmp(interface, "wl_data_device_manager") == 0) {
    struct sl_data_device_manager* data_device_manager =
        malloc(sizeof(struct sl_data_device_manager));
    assert(data_device_manager);
    data_device_manager->ctx = ctx;
    data_device_manager->id = id;
    data_device_manager->version = MIN(3, version);
    data_device_manager->internal = NULL;
    data_device_manager->host_global = NULL;
    assert(!ctx->data_device_manager);
    ctx->data_device_manager = data_device_manager;
    if (ctx->xwayland) {
      data_device_manager->internal =
          wl_registry_bind(registry, id, &wl_data_device_manager_interface,
                           data_device_manager->version);
    } else {
      data_device_manager->host_global =
          sl_data_device_manager_global_create(ctx);
    }
  } else if (strcmp(interface, "zxdg_shell_v6") == 0) {
    struct sl_xdg_shell* xdg_shell = malloc(sizeof(struct sl_xdg_shell));
    assert(xdg_shell);
    xdg_shell->ctx = ctx;
    xdg_shell->id = id;
    xdg_shell->internal = NULL;
    xdg_shell->host_global = NULL;
    assert(!ctx->xdg_shell);
    ctx->xdg_shell = xdg_shell;
    if (ctx->xwayland) {
      xdg_shell->internal =
          wl_registry_bind(registry, id, &zxdg_shell_v6_interface, 1);
      zxdg_shell_v6_add_listener(xdg_shell->internal,
                                 &sl_internal_xdg_shell_listener, NULL);
    } else {
      xdg_shell->host_global = sl_xdg_shell_global_create(ctx);
    }
  } else if (strcmp(interface, "zaura_shell") == 0) {
    if (version >= MIN_AURA_SHELL_VERSION) {
      struct sl_aura_shell* aura_shell = malloc(sizeof(struct sl_aura_shell));
      assert(aura_shell);
      aura_shell->ctx = ctx;
      aura_shell->id = id;
      aura_shell->version = MIN(MIN_AURA_SHELL_VERSION, version);
      aura_shell->host_gtk_shell_global = NULL;
      aura_shell->internal = wl_registry_bind(
          registry, id, &zaura_shell_interface, aura_shell->version);
      assert(!ctx->aura_shell);
      ctx->aura_shell = aura_shell;
      aura_shell->host_gtk_shell_global = sl_gtk_shell_global_create(ctx);
    }
  } else if (strcmp(interface, "wp_viewporter") == 0) {
    struct sl_viewporter* viewporter = malloc(sizeof(struct sl_viewporter));
    assert(viewporter);
    viewporter->ctx = ctx;
    viewporter->id = id;
    viewporter->host_viewporter_global = NULL;
    viewporter->internal =
        wl_registry_bind(registry, id, &wp_viewporter_interface, 1);
    assert(!ctx->viewporter);
    ctx->viewporter = viewporter;
    viewporter->host_viewporter_global = sl_viewporter_global_create(ctx);
    // Allow non-integer scale.
    ctx->scale = MIN(MAX_SCALE, MAX(MIN_SCALE, ctx->desired_scale));
  } else if (strcmp(interface, "zwp_linux_dmabuf_v1") == 0) {
    struct sl_linux_dmabuf* linux_dmabuf =
        malloc(sizeof(struct sl_linux_dmabuf));
    assert(linux_dmabuf);
    linux_dmabuf->ctx = ctx;
    linux_dmabuf->id = id;
    linux_dmabuf->version = MIN(2, version);
    linux_dmabuf->internal = wl_registry_bind(
        registry, id, &zwp_linux_dmabuf_v1_interface, linux_dmabuf->version);
    assert(!ctx->linux_dmabuf);
    ctx->linux_dmabuf = linux_dmabuf;
    linux_dmabuf->host_drm_global = sl_drm_global_create(ctx);
  } else if (strcmp(interface, "zcr_keyboard_extension_v1") == 0) {
    struct sl_keyboard_extension* keyboard_extension =
        malloc(sizeof(struct sl_keyboard_extension));
    assert(keyboard_extension);
    keyboard_extension->ctx = ctx;
    keyboard_extension->id = id;
    keyboard_extension->internal =
        wl_registry_bind(registry, id, &zcr_keyboard_extension_v1_interface, 1);
    assert(!ctx->keyboard_extension);
    ctx->keyboard_extension = keyboard_extension;
  } else if (strcmp(interface, "zwp_text_input_manager_v1") == 0) {
    struct sl_text_input_manager* text_input_manager =
        malloc(sizeof(struct sl_text_input_manager));
    assert(text_input_manager);
    text_input_manager->ctx = ctx;
    text_input_manager->id = id;
    text_input_manager->internal =
        wl_registry_bind(registry, id, &zwp_text_input_manager_v1_interface, 1);
    text_input_manager->host_global = sl_text_input_manager_global_create(ctx);
    assert(!ctx->text_input_manager);
    ctx->text_input_manager = text_input_manager;
  }
}

static void sl_registry_remover(void* data,
                                struct wl_registry* registry,
                                uint32_t id) {
  struct sl_context* ctx = (struct sl_context*)data;
  struct sl_output* output;
  struct sl_seat* seat;

  if (ctx->compositor && ctx->compositor->id == id) {
    sl_global_destroy(ctx->compositor->host_global);
    wl_compositor_destroy(ctx->compositor->internal);
    free(ctx->compositor);
    ctx->compositor = NULL;
    return;
  }
  if (ctx->subcompositor && ctx->subcompositor->id == id) {
    sl_global_destroy(ctx->subcompositor->host_global);
    wl_shm_destroy(ctx->shm->internal);
    free(ctx->subcompositor);
    ctx->subcompositor = NULL;
    return;
  }
  if (ctx->shm && ctx->shm->id == id) {
    sl_global_destroy(ctx->shm->host_global);
    free(ctx->shm);
    ctx->shm = NULL;
    return;
  }
  if (ctx->shell && ctx->shell->id == id) {
    sl_global_destroy(ctx->shell->host_global);
    free(ctx->shell);
    ctx->shell = NULL;
    return;
  }
  if (ctx->data_device_manager && ctx->data_device_manager->id == id) {
    if (ctx->data_device_manager->host_global)
      sl_global_destroy(ctx->data_device_manager->host_global);
    if (ctx->data_device_manager->internal)
      wl_data_device_manager_destroy(ctx->data_device_manager->internal);
    free(ctx->data_device_manager);
    ctx->data_device_manager = NULL;
    return;
  }
  if (ctx->xdg_shell && ctx->xdg_shell->id == id) {
    if (ctx->xdg_shell->host_global)
      sl_global_destroy(ctx->xdg_shell->host_global);
    if (ctx->xdg_shell->internal)
      zxdg_shell_v6_destroy(ctx->xdg_shell->internal);
    free(ctx->xdg_shell);
    ctx->xdg_shell = NULL;
    return;
  }
  if (ctx->aura_shell && ctx->aura_shell->id == id) {
    if (ctx->aura_shell->host_gtk_shell_global)
      sl_global_destroy(ctx->aura_shell->host_gtk_shell_global);
    zaura_shell_destroy(ctx->aura_shell->internal);
    free(ctx->aura_shell);
    ctx->aura_shell = NULL;
    return;
  }
  if (ctx->viewporter && ctx->viewporter->id == id) {
    if (ctx->viewporter->host_viewporter_global)
      sl_global_destroy(ctx->viewporter->host_viewporter_global);
    wp_viewporter_destroy(ctx->viewporter->internal);
    free(ctx->viewporter);
    ctx->viewporter = NULL;
    return;
  }
  if (ctx->linux_dmabuf && ctx->linux_dmabuf->id == id) {
    if (ctx->linux_dmabuf->host_drm_global)
      sl_global_destroy(ctx->linux_dmabuf->host_drm_global);
    zwp_linux_dmabuf_v1_destroy(ctx->linux_dmabuf->internal);
    free(ctx->linux_dmabuf);
    ctx->linux_dmabuf = NULL;
    return;
  }
  if (ctx->keyboard_extension && ctx->keyboard_extension->id == id) {
    zcr_keyboard_extension_v1_destroy(ctx->keyboard_extension->internal);
    free(ctx->keyboard_extension);
    ctx->keyboard_extension = NULL;
    return;
  }
  if (ctx->text_input_manager && ctx->text_input_manager->id == id) {
    sl_global_destroy(ctx->text_input_manager->host_global);
    free(ctx->text_input_manager);
    ctx->text_input_manager = NULL;
    return;
  }
  if (ctx->relative_pointer_manager &&
      ctx->relative_pointer_manager->id == id) {
    sl_global_destroy(ctx->relative_pointer_manager->host_global);
    free(ctx->relative_pointer_manager);
    ctx->relative_pointer_manager = NULL;
    return;
  }
  if (ctx->pointer_constraints && ctx->pointer_constraints->id == id) {
    sl_global_destroy(ctx->pointer_constraints->host_global);
    free(ctx->pointer_constraints);
    ctx->pointer_constraints = NULL;
    return;
  }
  wl_list_for_each(output, &ctx->outputs, link) {
    if (output->id == id) {
      sl_global_destroy(output->host_global);
      wl_list_remove(&output->link);
      free(output);
      return;
    }
  }
  wl_list_for_each(seat, &ctx->seats, link) {
    if (seat->id == id) {
      sl_global_destroy(seat->host_global);
      wl_list_remove(&seat->link);
      free(seat);
      return;
    }
  }

  // Not reached.
  assert(0);
}

static const struct wl_registry_listener sl_registry_listener = {
    sl_registry_handler, sl_registry_remover};

static int sl_handle_event(int fd, uint32_t mask, void* data) {
  struct sl_context* ctx = (struct sl_context*)data;
  int count = 0;

  if ((mask & WL_EVENT_HANGUP) || (mask & WL_EVENT_ERROR)) {
    wl_client_flush(ctx->client);
    exit(EXIT_SUCCESS);
  }

  if (mask & WL_EVENT_READABLE)
    count = wl_display_dispatch(ctx->display);
  if (mask & WL_EVENT_WRITABLE)
    wl_display_flush(ctx->display);

  if (mask == 0) {
    count = wl_display_dispatch_pending(ctx->display);
    wl_display_flush(ctx->display);
  }

  return count;
}

static void sl_create_window(struct sl_context* ctx,
                             xcb_window_t id,
                             int x,
                             int y,
                             int width,
                             int height,
                             int border_width) {
  struct sl_window* window = malloc(sizeof(struct sl_window));
  uint32_t values[1];
  assert(window);
  window->ctx = ctx;
  window->id = id;
  window->frame_id = XCB_WINDOW_NONE;
  window->host_surface_id = 0;
  window->unpaired = 1;
  window->x = x;
  window->y = y;
  window->width = width;
  window->height = height;
  window->border_width = border_width;
  window->depth = 0;
  window->managed = 0;
  window->realized = 0;
  window->activated = 0;
  window->maximized = 0;
  window->allow_resize = 1;
  window->transient_for = XCB_WINDOW_NONE;
  window->client_leader = XCB_WINDOW_NONE;
  window->decorated = 0;
  window->name = NULL;
  window->clazz = NULL;
  window->startup_id = NULL;
  window->dark_frame = 0;
  window->size_flags = P_POSITION;
  window->min_width = 0;
  window->min_height = 0;
  window->max_width = 0;
  window->max_height = 0;
  window->xdg_surface = NULL;
  window->xdg_toplevel = NULL;
  window->xdg_popup = NULL;
  window->aura_surface = NULL;
  window->next_config.serial = 0;
  window->next_config.mask = 0;
  window->next_config.states_length = 0;
  window->pending_config.serial = 0;
  window->pending_config.mask = 0;
  window->pending_config.states_length = 0;
  wl_list_insert(&ctx->unpaired_windows, &window->link);
  values[0] = XCB_EVENT_MASK_PROPERTY_CHANGE | XCB_EVENT_MASK_FOCUS_CHANGE;
  xcb_change_window_attributes(ctx->connection, window->id, XCB_CW_EVENT_MASK,
                               values);
}

static void sl_destroy_window(struct sl_window* window) {
  if (window->frame_id != XCB_WINDOW_NONE)
    xcb_destroy_window(window->ctx->connection, window->frame_id);

  if (window->ctx->host_focus_window == window) {
    window->ctx->host_focus_window = NULL;
    window->ctx->needs_set_input_focus = 1;
  }

  if (window->xdg_popup)
    zxdg_popup_v6_destroy(window->xdg_popup);
  if (window->xdg_toplevel)
    zxdg_toplevel_v6_destroy(window->xdg_toplevel);
  if (window->xdg_surface)
    zxdg_surface_v6_destroy(window->xdg_surface);
  if (window->aura_surface)
    zaura_surface_destroy(window->aura_surface);

  if (window->name)
    free(window->name);
  if (window->clazz)
    free(window->clazz);
  if (window->startup_id)
    free(window->startup_id);

  wl_list_remove(&window->link);
  free(window);
}

static int sl_is_window(struct sl_window* window, xcb_window_t id) {
  if (window->id == id)
    return 1;

  if (window->frame_id != XCB_WINDOW_NONE) {
    if (window->frame_id == id)
      return 1;
  }

  return 0;
}

static struct sl_window* sl_lookup_window(struct sl_context* ctx,
                                          xcb_window_t id) {
  struct sl_window* window;

  wl_list_for_each(window, &ctx->windows, link) {
    if (sl_is_window(window, id))
      return window;
  }
  wl_list_for_each(window, &ctx->unpaired_windows, link) {
    if (sl_is_window(window, id))
      return window;
  }
  return NULL;
}

static int sl_is_our_window(struct sl_context* ctx, xcb_window_t id) {
  const xcb_setup_t* setup = xcb_get_setup(ctx->connection);

  return (id & ~setup->resource_id_mask) == setup->resource_id_base;
}

static void sl_handle_create_notify(struct sl_context* ctx,
                                    xcb_create_notify_event_t* event) {
  if (sl_is_our_window(ctx, event->window))
    return;

  sl_create_window(ctx, event->window, event->x, event->y, event->width,
                   event->height, event->border_width);
}

static void sl_handle_destroy_notify(struct sl_context* ctx,
                                     xcb_destroy_notify_event_t* event) {
  struct sl_window* window;

  if (sl_is_our_window(ctx, event->window))
    return;

  window = sl_lookup_window(ctx, event->window);
  if (!window)
    return;

  sl_destroy_window(window);
}

static void sl_handle_reparent_notify(struct sl_context* ctx,
                                      xcb_reparent_notify_event_t* event) {
  struct sl_window* window;

  if (event->parent == ctx->screen->root) {
    int width = 1;
    int height = 1;
    int border_width = 0;

    // Return early if window is already tracked. This happens when we
    // reparent an unampped window back to the root window.
    window = sl_lookup_window(ctx, event->window);
    if (window)
      return;

    xcb_get_geometry_reply_t* geometry_reply = xcb_get_geometry_reply(
        ctx->connection, xcb_get_geometry(ctx->connection, event->window),
        NULL);

    if (geometry_reply) {
      width = geometry_reply->width;
      height = geometry_reply->height;
      border_width = geometry_reply->border_width;
      free(geometry_reply);
    }
    sl_create_window(ctx, event->window, event->x, event->y, width, height,
                     border_width);
    return;
  }

  if (sl_is_our_window(ctx, event->parent))
    return;

  window = sl_lookup_window(ctx, event->window);
  if (!window)
    return;

  sl_destroy_window(window);
}

static void sl_handle_map_request(struct sl_context* ctx,
                                  xcb_map_request_event_t* event) {
  struct sl_window* window = sl_lookup_window(ctx, event->window);
  struct {
    int type;
    xcb_atom_t atom;
  } properties[] = {
      {PROPERTY_WM_NAME, XCB_ATOM_WM_NAME},
      {PROPERTY_WM_CLASS, XCB_ATOM_WM_CLASS},
      {PROPERTY_WM_TRANSIENT_FOR, XCB_ATOM_WM_TRANSIENT_FOR},
      {PROPERTY_WM_NORMAL_HINTS, XCB_ATOM_WM_NORMAL_HINTS},
      {PROPERTY_WM_CLIENT_LEADER, ctx->atoms[ATOM_WM_CLIENT_LEADER].value},
      {PROPERTY_MOTIF_WM_HINTS, ctx->atoms[ATOM_MOTIF_WM_HINTS].value},
      {PROPERTY_NET_STARTUP_ID, ctx->atoms[ATOM_NET_STARTUP_ID].value},
      {PROPERTY_NET_WM_STATE, ctx->atoms[ATOM_NET_WM_STATE].value},
      {PROPERTY_GTK_THEME_VARIANT, ctx->atoms[ATOM_GTK_THEME_VARIANT].value},
  };
  xcb_get_geometry_cookie_t geometry_cookie;
  xcb_get_property_cookie_t property_cookies[ARRAY_SIZE(properties)];
  struct sl_wm_size_hints size_hints = {0};
  struct sl_mwm_hints mwm_hints = {0};
  xcb_atom_t* reply_atoms;
  bool maximize_h = false, maximize_v = false;
  uint32_t values[5];
  int i;

  if (!window)
    return;

  assert(!sl_is_our_window(ctx, event->window));

  window->managed = 1;
  if (window->frame_id == XCB_WINDOW_NONE)
    geometry_cookie = xcb_get_geometry(ctx->connection, window->id);

  for (i = 0; i < ARRAY_SIZE(properties); ++i) {
    property_cookies[i] =
        xcb_get_property(ctx->connection, 0, window->id, properties[i].atom,
                         XCB_ATOM_ANY, 0, 2048);
  }

  if (window->frame_id == XCB_WINDOW_NONE) {
    xcb_get_geometry_reply_t* geometry_reply =
        xcb_get_geometry_reply(ctx->connection, geometry_cookie, NULL);
    if (geometry_reply) {
      window->x = geometry_reply->x;
      window->y = geometry_reply->y;
      window->width = geometry_reply->width;
      window->height = geometry_reply->height;
      window->depth = geometry_reply->depth;
      free(geometry_reply);
    }
  }

  free(window->name);
  window->name = NULL;
  free(window->clazz);
  window->clazz = NULL;
  free(window->startup_id);
  window->startup_id = NULL;
  window->transient_for = XCB_WINDOW_NONE;
  window->client_leader = XCB_WINDOW_NONE;
  window->decorated = 1;
  window->size_flags = 0;
  window->dark_frame = 0;

  for (i = 0; i < ARRAY_SIZE(properties); ++i) {
    xcb_get_property_reply_t* reply =
        xcb_get_property_reply(ctx->connection, property_cookies[i], NULL);

    if (!reply)
      continue;

    if (reply->type == XCB_ATOM_NONE) {
      free(reply);
      continue;
    }

    switch (properties[i].type) {
      case PROPERTY_WM_NAME:
        window->name = strndup(xcb_get_property_value(reply),
                               xcb_get_property_value_length(reply));
        break;
      case PROPERTY_WM_CLASS: {
        // WM_CLASS property contains two consecutive null-terminated strings.
        // These specify the Instance and Class names. If a global app ID is
        // not set then use Class name for app ID.
        const char* value = xcb_get_property_value(reply);
        int value_length = xcb_get_property_value_length(reply);
        int instance_length = strnlen(value, value_length);
        if (value_length > instance_length) {
          window->clazz = strndup(value + instance_length + 1,
                                  value_length - instance_length - 1);
        }
      } break;
      case PROPERTY_WM_TRANSIENT_FOR:
        if (xcb_get_property_value_length(reply) >= 4)
          window->transient_for = *((uint32_t*)xcb_get_property_value(reply));
        break;
      case PROPERTY_WM_NORMAL_HINTS:
        if (xcb_get_property_value_length(reply) >= sizeof(size_hints))
          memcpy(&size_hints, xcb_get_property_value(reply),
                 sizeof(size_hints));
        break;
      case PROPERTY_WM_CLIENT_LEADER:
        if (xcb_get_property_value_length(reply) >= 4)
          window->client_leader = *((uint32_t*)xcb_get_property_value(reply));
        break;
      case PROPERTY_MOTIF_WM_HINTS:
        if (xcb_get_property_value_length(reply) >= sizeof(mwm_hints))
          memcpy(&mwm_hints, xcb_get_property_value(reply), sizeof(mwm_hints));
        break;
      case PROPERTY_NET_STARTUP_ID:
        window->startup_id = strndup(xcb_get_property_value(reply),
                                     xcb_get_property_value_length(reply));
        break;
      case PROPERTY_NET_WM_STATE:
        reply_atoms = xcb_get_property_value(reply);
        for (i = 0;
             i < xcb_get_property_value_length(reply) / sizeof(xcb_atom_t);
             ++i) {
          if (reply_atoms[i] ==
              ctx->atoms[ATOM_NET_WM_STATE_MAXIMIZED_HORZ].value) {
            maximize_h = true;
          } else if (reply_atoms[i] ==
                     ctx->atoms[ATOM_NET_WM_STATE_MAXIMIZED_VERT].value) {
            maximize_v = true;
          }
        }
        // Neither wayland not CrOS support 1D maximizing, so sommelier will
        // only consider a window maximized if both dimensions are. This
        // behaviour is consistent with sl_handle_client_message().
        window->maximized = maximize_h && maximize_v;
        break;
      case PROPERTY_GTK_THEME_VARIANT:
        if (xcb_get_property_value_length(reply) >= 4)
          window->dark_frame = !strcmp(xcb_get_property_value(reply), "dark");
        break;
      default:
        break;
    }
    free(reply);
  }

  if (mwm_hints.flags & MWM_HINTS_DECORATIONS) {
    if (mwm_hints.decorations & MWM_DECOR_ALL)
      window->decorated = ~mwm_hints.decorations & MWM_DECOR_TITLE;
    else
      window->decorated = mwm_hints.decorations & MWM_DECOR_TITLE;
  }

  // Allow user/program controlled position for transients.
  if (window->transient_for)
    window->size_flags |= size_hints.flags & (US_POSITION | P_POSITION);

  // If startup ID is not set, then try the client leader window.
  if (!window->startup_id && window->client_leader) {
    xcb_get_property_reply_t* reply = xcb_get_property_reply(
        ctx->connection,
        xcb_get_property(ctx->connection, 0, window->client_leader,
                         ctx->atoms[ATOM_NET_STARTUP_ID].value, XCB_ATOM_ANY, 0,
                         2048),
        NULL);
    if (reply) {
      if (reply->type != XCB_ATOM_NONE) {
        window->startup_id = strndup(xcb_get_property_value(reply),
                                     xcb_get_property_value_length(reply));
      }
      free(reply);
    }
  }

  window->size_flags |= size_hints.flags & (P_MIN_SIZE | P_MAX_SIZE);
  if (window->size_flags & P_MIN_SIZE) {
    window->min_width = size_hints.min_width;
    window->min_height = size_hints.min_height;
  }
  if (window->size_flags & P_MAX_SIZE) {
    window->max_width = size_hints.max_width;
    window->max_height = size_hints.max_height;
  }

  window->border_width = 0;
  sl_adjust_window_size_for_screen_size(window);
  if (!(window->size_flags & (US_POSITION | P_POSITION)))
    sl_adjust_window_position_for_screen_size(window);

  values[0] = window->width;
  values[1] = window->height;
  values[2] = 0;
  xcb_configure_window(ctx->connection, window->id,
                       XCB_CONFIG_WINDOW_WIDTH | XCB_CONFIG_WINDOW_HEIGHT |
                           XCB_CONFIG_WINDOW_BORDER_WIDTH,
                       values);
  // This needs to match the frame extents of the X11 frame window used
  // for reparenting or applications tend to be confused. The actual window
  // frame size used by the host compositor can be different.
  values[0] = 0;
  values[1] = 0;
  values[2] = 0;
  values[3] = 0;
  xcb_change_property(ctx->connection, XCB_PROP_MODE_REPLACE, window->id,
                      ctx->atoms[ATOM_NET_FRAME_EXTENTS].value,
                      XCB_ATOM_CARDINAL, 32, 4, values);

  // Remove weird gravities.
  values[0] = XCB_GRAVITY_NORTH_WEST;
  xcb_change_window_attributes(ctx->connection, window->id, XCB_CW_WIN_GRAVITY,
                               values);

  if (window->frame_id == XCB_WINDOW_NONE) {
    int depth = window->depth ? window->depth : ctx->screen->root_depth;

    values[0] = ctx->screen->black_pixel;
    values[1] = XCB_EVENT_MASK_SUBSTRUCTURE_NOTIFY |
                XCB_EVENT_MASK_SUBSTRUCTURE_REDIRECT;
    values[2] = ctx->colormaps[depth];

    window->frame_id = xcb_generate_id(ctx->connection);
    xcb_create_window(
        ctx->connection, depth, window->frame_id, ctx->screen->root, window->x,
        window->y, window->width, window->height, 0,
        XCB_WINDOW_CLASS_INPUT_OUTPUT, ctx->visual_ids[depth],
        XCB_CW_BORDER_PIXEL | XCB_CW_EVENT_MASK | XCB_CW_COLORMAP, values);
    values[0] = XCB_STACK_MODE_BELOW;
    xcb_configure_window(ctx->connection, window->frame_id,
                         XCB_CONFIG_WINDOW_STACK_MODE, values);
    xcb_reparent_window(ctx->connection, window->id, window->frame_id, 0, 0);
  } else {
    values[0] = window->x;
    values[1] = window->y;
    values[2] = window->width;
    values[3] = window->height;
    values[4] = XCB_STACK_MODE_BELOW;
    xcb_configure_window(
        ctx->connection, window->frame_id,
        XCB_CONFIG_WINDOW_X | XCB_CONFIG_WINDOW_Y | XCB_CONFIG_WINDOW_WIDTH |
            XCB_CONFIG_WINDOW_HEIGHT | XCB_CONFIG_WINDOW_STACK_MODE,
        values);
  }

  sl_window_set_wm_state(window, WM_STATE_NORMAL);
  sl_send_configure_notify(window);

  xcb_map_window(ctx->connection, window->id);
  xcb_map_window(ctx->connection, window->frame_id);
}

static void sl_handle_map_notify(struct sl_context* ctx,
                                 xcb_map_notify_event_t* event) {}

static void sl_handle_unmap_notify(struct sl_context* ctx,
                                   xcb_unmap_notify_event_t* event) {
  struct sl_window* window;

  if (sl_is_our_window(ctx, event->window))
    return;

  if (event->response_type & SEND_EVENT_MASK)
    return;

  window = sl_lookup_window(ctx, event->window);
  if (!window)
    return;

  if (ctx->host_focus_window == window) {
    ctx->host_focus_window = NULL;
    ctx->needs_set_input_focus = 1;
  }

  if (window->host_surface_id) {
    window->host_surface_id = 0;
    sl_window_update(window);
  }

  sl_window_set_wm_state(window, WM_STATE_WITHDRAWN);

  // Reparent window and destroy frame if it exists.
  if (window->frame_id != XCB_WINDOW_NONE) {
    xcb_reparent_window(ctx->connection, window->id, ctx->screen->root,
                        window->x, window->y);
    xcb_destroy_window(ctx->connection, window->frame_id);
    window->frame_id = XCB_WINDOW_NONE;
  }

  // Reset properties to unmanaged state in case the window transitions to
  // an override-redirect window.
  window->managed = 0;
  window->decorated = 0;
  window->size_flags = P_POSITION;
}

static void sl_handle_configure_request(struct sl_context* ctx,
                                        xcb_configure_request_event_t* event) {
  struct sl_window* window = sl_lookup_window(ctx, event->window);
  int width = window->width;
  int height = window->height;
  uint32_t values[7];

  assert(!sl_is_our_window(ctx, event->window));

  if (!window->managed) {
    int i = 0;

    if (event->value_mask & XCB_CONFIG_WINDOW_X)
      values[i++] = event->x;
    if (event->value_mask & XCB_CONFIG_WINDOW_Y)
      values[i++] = event->y;
    if (event->value_mask & XCB_CONFIG_WINDOW_WIDTH)
      values[i++] = event->width;
    if (event->value_mask & XCB_CONFIG_WINDOW_HEIGHT)
      values[i++] = event->height;
    if (event->value_mask & XCB_CONFIG_WINDOW_BORDER_WIDTH)
      values[i++] = event->border_width;
    if (event->value_mask & XCB_CONFIG_WINDOW_SIBLING)
      values[i++] = event->sibling;
    if (event->value_mask & XCB_CONFIG_WINDOW_STACK_MODE)
      values[i++] = event->stack_mode;

    xcb_configure_window(ctx->connection, window->id, event->value_mask,
                         values);
    return;
  }

  // Ack configure events as satisfying request removes the guarantee
  // that matching contents will arrive.
  if (window->xdg_toplevel) {
    if (window->pending_config.serial) {
      zxdg_surface_v6_ack_configure(window->xdg_surface,
                                    window->pending_config.serial);
      window->pending_config.serial = 0;
      window->pending_config.mask = 0;
      window->pending_config.states_length = 0;
    }
    if (window->next_config.serial) {
      zxdg_surface_v6_ack_configure(window->xdg_surface,
                                    window->next_config.serial);
      window->next_config.serial = 0;
      window->next_config.mask = 0;
      window->next_config.states_length = 0;
    }
  }

  if (event->value_mask & XCB_CONFIG_WINDOW_X)
    window->x = event->x;
  if (event->value_mask & XCB_CONFIG_WINDOW_Y)
    window->y = event->y;

  if (window->allow_resize) {
    if (event->value_mask & XCB_CONFIG_WINDOW_WIDTH)
      window->width = event->width;
    if (event->value_mask & XCB_CONFIG_WINDOW_HEIGHT)
      window->height = event->height;
  }

  sl_adjust_window_size_for_screen_size(window);
  if (window->size_flags & (US_POSITION | P_POSITION))
    sl_window_update(window);
  else
    sl_adjust_window_position_for_screen_size(window);

  values[0] = window->x;
  values[1] = window->y;
  values[2] = window->width;
  values[3] = window->height;
  values[4] = 0;
  xcb_configure_window(ctx->connection, window->frame_id,
                       XCB_CONFIG_WINDOW_X | XCB_CONFIG_WINDOW_Y |
                           XCB_CONFIG_WINDOW_WIDTH | XCB_CONFIG_WINDOW_HEIGHT,
                       values);

  // We need to send a synthetic configure notify if:
  // - Not changing the size, location, border width.
  // - Moving the window without resizing it or changing its border width.
  if (width != window->width || height != window->height ||
      window->border_width) {
    xcb_configure_window(ctx->connection, window->id,
                         XCB_CONFIG_WINDOW_WIDTH | XCB_CONFIG_WINDOW_HEIGHT |
                             XCB_CONFIG_WINDOW_BORDER_WIDTH,
                         &values[2]);
    window->border_width = 0;
  } else {
    sl_send_configure_notify(window);
  }
}

static void sl_handle_configure_notify(struct sl_context* ctx,
                                       xcb_configure_notify_event_t* event) {
  struct sl_window* window;

  if (sl_is_our_window(ctx, event->window))
    return;

  if (event->window == ctx->screen->root) {
    xcb_get_geometry_reply_t* geometry_reply = xcb_get_geometry_reply(
        ctx->connection, xcb_get_geometry(ctx->connection, event->window),
        NULL);
    int width = ctx->screen->width_in_pixels;
    int height = ctx->screen->height_in_pixels;

    if (geometry_reply) {
      width = geometry_reply->width;
      height = geometry_reply->height;
      free(geometry_reply);
    }

    if (width == ctx->screen->width_in_pixels ||
        height == ctx->screen->height_in_pixels) {
      return;
    }

    ctx->screen->width_in_pixels = width;
    ctx->screen->height_in_pixels = height;

    // Re-center managed windows.
    wl_list_for_each(window, &ctx->windows, link) {
      int x, y;

      if (window->size_flags & (US_POSITION | P_POSITION))
        continue;

      x = window->x;
      y = window->y;
      sl_adjust_window_position_for_screen_size(window);
      if (window->x != x || window->y != y) {
        uint32_t values[2];

        values[0] = window->x;
        values[1] = window->y;
        xcb_configure_window(ctx->connection, window->frame_id,
                             XCB_CONFIG_WINDOW_X | XCB_CONFIG_WINDOW_Y, values);
        sl_send_configure_notify(window);
      }
    }
    return;
  }

  window = sl_lookup_window(ctx, event->window);
  if (!window)
    return;

  if (window->managed)
    return;

  window->width = event->width;
  window->height = event->height;
  window->border_width = event->border_width;
  if (event->x != window->x || event->y != window->y) {
    window->x = event->x;
    window->y = event->y;
    sl_window_update(window);
  }
}

static uint32_t sl_resize_edge(int net_wm_moveresize_size) {
  switch (net_wm_moveresize_size) {
    case NET_WM_MOVERESIZE_SIZE_TOPLEFT:
      return ZXDG_TOPLEVEL_V6_RESIZE_EDGE_TOP_LEFT;
    case NET_WM_MOVERESIZE_SIZE_TOP:
      return ZXDG_TOPLEVEL_V6_RESIZE_EDGE_TOP;
    case NET_WM_MOVERESIZE_SIZE_TOPRIGHT:
      return ZXDG_TOPLEVEL_V6_RESIZE_EDGE_TOP_RIGHT;
    case NET_WM_MOVERESIZE_SIZE_RIGHT:
      return ZXDG_TOPLEVEL_V6_RESIZE_EDGE_RIGHT;
    case NET_WM_MOVERESIZE_SIZE_BOTTOMRIGHT:
      return ZXDG_TOPLEVEL_V6_RESIZE_EDGE_BOTTOM_RIGHT;
    case NET_WM_MOVERESIZE_SIZE_BOTTOM:
      return ZXDG_TOPLEVEL_V6_RESIZE_EDGE_BOTTOM;
    case NET_WM_MOVERESIZE_SIZE_BOTTOMLEFT:
      return ZXDG_TOPLEVEL_V6_RESIZE_EDGE_BOTTOM_LEFT;
    case NET_WM_MOVERESIZE_SIZE_LEFT:
      return ZXDG_TOPLEVEL_V6_RESIZE_EDGE_LEFT;
    default:
      return ZXDG_TOPLEVEL_V6_RESIZE_EDGE_NONE;
  }
}

static void sl_handle_client_message(struct sl_context* ctx,
                                     xcb_client_message_event_t* event) {
  if (event->type == ctx->atoms[ATOM_WL_SURFACE_ID].value) {
    struct sl_window *window, *unpaired_window = NULL;

    wl_list_for_each(window, &ctx->unpaired_windows, link) {
      if (sl_is_window(window, event->window)) {
        unpaired_window = window;
        break;
      }
    }

    if (unpaired_window) {
      unpaired_window->host_surface_id = event->data.data32[0];
      sl_window_update(unpaired_window);
    }
  } else if (event->type == ctx->atoms[ATOM_NET_WM_MOVERESIZE].value) {
    struct sl_window* window = sl_lookup_window(ctx, event->window);

    if (window && window->xdg_toplevel) {
      struct sl_host_seat* seat = window->ctx->default_seat;

      if (!seat)
        return;

      if (event->data.data32[2] == NET_WM_MOVERESIZE_MOVE) {
        zxdg_toplevel_v6_move(window->xdg_toplevel, seat->proxy,
                              seat->seat->last_serial);
      } else {
        uint32_t edge = sl_resize_edge(event->data.data32[2]);

        if (edge == ZXDG_TOPLEVEL_V6_RESIZE_EDGE_NONE)
          return;

        zxdg_toplevel_v6_resize(window->xdg_toplevel, seat->proxy,
                                seat->seat->last_serial, edge);
      }
    }
  } else if (event->type == ctx->atoms[ATOM_NET_WM_STATE].value) {
    struct sl_window* window = sl_lookup_window(ctx, event->window);

    if (window && window->xdg_toplevel) {
      int changed[ATOM_LAST + 1];
      uint32_t action = event->data.data32[0];
      int i;

      for (i = 0; i < ARRAY_SIZE(ctx->atoms); ++i) {
        changed[i] = event->data.data32[1] == ctx->atoms[i].value ||
                     event->data.data32[2] == ctx->atoms[i].value;
      }

      if (changed[ATOM_NET_WM_STATE_FULLSCREEN]) {
        if (action == NET_WM_STATE_ADD)
          zxdg_toplevel_v6_set_fullscreen(window->xdg_toplevel, NULL);
        else if (action == NET_WM_STATE_REMOVE)
          zxdg_toplevel_v6_unset_fullscreen(window->xdg_toplevel);
      }

      if (changed[ATOM_NET_WM_STATE_MAXIMIZED_VERT] &&
          changed[ATOM_NET_WM_STATE_MAXIMIZED_HORZ]) {
        if (action == NET_WM_STATE_ADD)
          zxdg_toplevel_v6_set_maximized(window->xdg_toplevel);
        else if (action == NET_WM_STATE_REMOVE)
          zxdg_toplevel_v6_unset_maximized(window->xdg_toplevel);
      }
    }
  } else if (event->type == ctx->atoms[ATOM_WM_CHANGE_STATE].value &&
             event->data.data32[0] == WM_STATE_ICONIC) {
    struct sl_window* window = sl_lookup_window(ctx, event->window);
    if (window && window->xdg_toplevel) {
      zxdg_toplevel_v6_set_minimized(window->xdg_toplevel);
    }
  }
}

static void sl_handle_focus_in(struct sl_context* ctx,
                               xcb_focus_in_event_t* event) {
  struct sl_window* window = sl_lookup_window(ctx, event->event);
  if (window && window->transient_for != XCB_WINDOW_NONE) {
    // Set our parent now as it might not have been set properly when the
    // window was realized.
    struct sl_window* parent = sl_lookup_window(ctx, window->transient_for);
    if (parent && parent->xdg_toplevel && window->xdg_toplevel)
      zxdg_toplevel_v6_set_parent(window->xdg_toplevel, parent->xdg_toplevel);
  }
}

static void sl_handle_focus_out(struct sl_context* ctx,
                                xcb_focus_out_event_t* event) {}

int sl_begin_data_source_send(struct sl_context* ctx,
                              int fd,
                              xcb_intern_atom_cookie_t cookie,
                              struct sl_data_source* data_source) {
  xcb_intern_atom_reply_t* reply =
      xcb_intern_atom_reply(ctx->connection, cookie, NULL);

  if (!reply) {
    close(fd);
    return 0;
  }

  int flags, rv;

  xcb_convert_selection(ctx->connection, ctx->selection_window,
                        ctx->atoms[ATOM_CLIPBOARD].value, reply->atom,
                        ctx->atoms[ATOM_WL_SELECTION].value, XCB_CURRENT_TIME);

  flags = fcntl(fd, F_GETFL, 0);
  rv = fcntl(fd, F_SETFL, flags | O_NONBLOCK);
  assert(!rv);
  UNUSED(rv);

  ctx->selection_data_source_send_fd = fd;
  free(reply);
  return 1;
}

void sl_process_data_source_send_pending_list(struct sl_context* ctx) {
  while (!wl_list_empty(&ctx->selection_data_source_send_pending)) {
    struct wl_list* next = ctx->selection_data_source_send_pending.next;
    struct sl_data_source_send_request* request;
    request = wl_container_of(next, request, link);
    wl_list_remove(next);

    int rv = sl_begin_data_source_send(ctx, request->fd, request->cookie,
                                       request->data_source);
    free(request);
    if (rv)
      break;
  }
}

static int sl_handle_selection_fd_writable(int fd, uint32_t mask, void* data) {
  struct sl_context* ctx = data;
  uint8_t* value;
  int bytes, bytes_left;

  value = xcb_get_property_value(ctx->selection_property_reply);
  bytes_left = xcb_get_property_value_length(ctx->selection_property_reply) -
               ctx->selection_property_offset;

  bytes = write(fd, value + ctx->selection_property_offset, bytes_left);
  if (bytes == -1) {
    fprintf(stderr, "write error to target fd: %m\n");
    close(fd);
    fd = -1;
  } else if (bytes == bytes_left) {
    if (ctx->selection_incremental_transfer) {
      xcb_delete_property(ctx->connection, ctx->selection_window,
                          ctx->atoms[ATOM_WL_SELECTION].value);
    } else {
      close(fd);
      fd = -1;
    }
  } else {
    ctx->selection_property_offset += bytes;
    return 1;
  }

  free(ctx->selection_property_reply);
  ctx->selection_property_reply = NULL;
  if (ctx->selection_send_event_source) {
    wl_event_source_remove(ctx->selection_send_event_source);
    ctx->selection_send_event_source = NULL;
  }
  if (fd < 0) {
    ctx->selection_data_source_send_fd = -1;
    sl_process_data_source_send_pending_list(ctx);
  }
  return 1;
}

static void sl_write_selection_property(struct sl_context* ctx,
                                        xcb_get_property_reply_t* reply) {
  ctx->selection_property_offset = 0;
  ctx->selection_property_reply = reply;
  sl_handle_selection_fd_writable(ctx->selection_data_source_send_fd,
                                  WL_EVENT_WRITABLE, ctx);

  if (!ctx->selection_property_reply)
    return;

  assert(!ctx->selection_send_event_source);
  ctx->selection_send_event_source = wl_event_loop_add_fd(
      wl_display_get_event_loop(ctx->host_display),
      ctx->selection_data_source_send_fd, WL_EVENT_WRITABLE,
      sl_handle_selection_fd_writable, ctx);
}

static void sl_send_selection_notify(struct sl_context* ctx,
                                     xcb_atom_t property) {
  xcb_selection_notify_event_t event = {
      .response_type = XCB_SELECTION_NOTIFY,
      .sequence = 0,
      .time = ctx->selection_request.time,
      .requestor = ctx->selection_request.requestor,
      .selection = ctx->selection_request.selection,
      .target = ctx->selection_request.target,
      .property = property,
      .pad0 = 0};

  xcb_send_event(ctx->connection, 0, ctx->selection_request.requestor,
                 XCB_EVENT_MASK_NO_EVENT, (char*)&event);
}

static void sl_send_selection_data(struct sl_context* ctx) {
  assert(!ctx->selection_data_ack_pending);
  xcb_change_property(
      ctx->connection, XCB_PROP_MODE_REPLACE, ctx->selection_request.requestor,
      ctx->selection_request.property, ctx->selection_data_type,
      /*format=*/8, ctx->selection_data.size, ctx->selection_data.data);
  ctx->selection_data_ack_pending = 1;
  ctx->selection_data.size = 0;
}

static const uint32_t sl_incr_chunk_size = 64 * 1024;

static int sl_handle_selection_fd_readable(int fd, uint32_t mask, void* data) {
  struct sl_context* ctx = data;
  int bytes, offset, bytes_left;
  void* p;

  offset = ctx->selection_data.size;
  if (ctx->selection_data.size < sl_incr_chunk_size)
    p = wl_array_add(&ctx->selection_data, sl_incr_chunk_size);
  else
    p = (char*)ctx->selection_data.data + ctx->selection_data.size;
  bytes_left = ctx->selection_data.alloc - offset;

  bytes = read(fd, p, bytes_left);
  if (bytes == -1) {
    fprintf(stderr, "read error from data source: %m\n");
    sl_send_selection_notify(ctx, XCB_ATOM_NONE);
    ctx->selection_data_offer_receive_fd = -1;
    close(fd);
  } else {
    ctx->selection_data.size = offset + bytes;
    if (ctx->selection_data.size >= sl_incr_chunk_size) {
      if (!ctx->selection_incremental_transfer) {
        ctx->selection_incremental_transfer = 1;
        xcb_change_property(
            ctx->connection, XCB_PROP_MODE_REPLACE,
            ctx->selection_request.requestor, ctx->selection_request.property,
            ctx->atoms[ATOM_INCR].value, 32, 1, &sl_incr_chunk_size);
        ctx->selection_data_ack_pending = 1;
        sl_send_selection_notify(ctx, ctx->selection_request.property);
      } else if (!ctx->selection_data_ack_pending) {
        sl_send_selection_data(ctx);
      }
    } else if (bytes == 0) {
      if (!ctx->selection_data_ack_pending)
        sl_send_selection_data(ctx);
      if (!ctx->selection_incremental_transfer) {
        sl_send_selection_notify(ctx, ctx->selection_request.property);
        ctx->selection_request.requestor = XCB_NONE;
        wl_array_release(&ctx->selection_data);
      }
      xcb_flush(ctx->connection);
      ctx->selection_data_offer_receive_fd = -1;
      close(fd);
    } else {
      ctx->selection_data.size = offset + bytes;
      return 1;
    }
  }

  wl_event_source_remove(ctx->selection_event_source);
  ctx->selection_event_source = NULL;
  return 1;
}

static void sl_handle_property_notify(struct sl_context* ctx,
                                      xcb_property_notify_event_t* event) {
  if (event->atom == XCB_ATOM_WM_NAME) {
    struct sl_window* window = sl_lookup_window(ctx, event->window);
    if (!window)
      return;

    if (window->name) {
      free(window->name);
      window->name = NULL;
    }

    if (event->state != XCB_PROPERTY_DELETE) {
      xcb_get_property_reply_t* reply = xcb_get_property_reply(
          ctx->connection,
          xcb_get_property(ctx->connection, 0, window->id, XCB_ATOM_WM_NAME,
                           XCB_ATOM_ANY, 0, 2048),
          NULL);
      if (reply) {
        window->name = strndup(xcb_get_property_value(reply),
                               xcb_get_property_value_length(reply));
        free(reply);
      }
    }

    if (!window->xdg_toplevel)
      return;

    if (window->name) {
      zxdg_toplevel_v6_set_title(window->xdg_toplevel, window->name);
    } else {
      zxdg_toplevel_v6_set_title(window->xdg_toplevel, "");
    }
  } else if (event->atom == XCB_ATOM_WM_NORMAL_HINTS) {
    struct sl_window* window = sl_lookup_window(ctx, event->window);
    if (!window)
      return;

    window->size_flags &= ~(P_MIN_SIZE | P_MAX_SIZE);

    if (event->state != XCB_PROPERTY_DELETE) {
      struct sl_wm_size_hints size_hints = {0};
      xcb_get_property_reply_t* reply = xcb_get_property_reply(
          ctx->connection,
          xcb_get_property(ctx->connection, 0, window->id,
                           XCB_ATOM_WM_NORMAL_HINTS, XCB_ATOM_ANY, 0,
                           sizeof(size_hints)),
          NULL);
      if (reply) {
        memcpy(&size_hints, xcb_get_property_value(reply), sizeof(size_hints));
        free(reply);
      }

      window->size_flags |= size_hints.flags & (P_MIN_SIZE | P_MAX_SIZE);
      if (window->size_flags & P_MIN_SIZE) {
        window->min_width = size_hints.min_width;
        window->min_height = size_hints.min_height;
      }
      if (window->size_flags & P_MAX_SIZE) {
        window->max_width = size_hints.max_width;
        window->max_height = size_hints.max_height;
      }
    }

    if (!window->xdg_toplevel)
      return;

    if (window->size_flags & P_MIN_SIZE) {
      zxdg_toplevel_v6_set_min_size(window->xdg_toplevel,
                                    window->min_width / ctx->scale,
                                    window->min_height / ctx->scale);
    } else {
      zxdg_toplevel_v6_set_min_size(window->xdg_toplevel, 0, 0);
    }

    if (window->size_flags & P_MAX_SIZE) {
      zxdg_toplevel_v6_set_max_size(window->xdg_toplevel,
                                    window->max_width / ctx->scale,
                                    window->max_height / ctx->scale);
    } else {
      zxdg_toplevel_v6_set_max_size(window->xdg_toplevel, 0, 0);
    }
  } else if (event->atom == ctx->atoms[ATOM_MOTIF_WM_HINTS].value) {
    struct sl_window* window = sl_lookup_window(ctx, event->window);
    if (!window)
      return;

    // Managed windows are decorated by default.
    window->decorated = window->managed;

    if (event->state != XCB_PROPERTY_DELETE) {
      struct sl_mwm_hints mwm_hints = {0};
      xcb_get_property_reply_t* reply = xcb_get_property_reply(
          ctx->connection,
          xcb_get_property(ctx->connection, 0, window->id,
                           ctx->atoms[ATOM_MOTIF_WM_HINTS].value, XCB_ATOM_ANY,
                           0, sizeof(mwm_hints)),
          NULL);
      if (reply) {
        if (mwm_hints.flags & MWM_HINTS_DECORATIONS) {
          if (mwm_hints.decorations & MWM_DECOR_ALL)
            window->decorated = ~mwm_hints.decorations & MWM_DECOR_TITLE;
          else
            window->decorated = mwm_hints.decorations & MWM_DECOR_TITLE;
        }
      }
    }

    if (!window->aura_surface)
      return;

    zaura_surface_set_frame(window->aura_surface,
                            window->decorated
                                ? ZAURA_SURFACE_FRAME_TYPE_NORMAL
                                : window->depth == 32
                                      ? ZAURA_SURFACE_FRAME_TYPE_NONE
                                      : ZAURA_SURFACE_FRAME_TYPE_SHADOW);
  } else if (event->atom == ctx->atoms[ATOM_GTK_THEME_VARIANT].value) {
    struct sl_window* window;
    uint32_t frame_color;

    window = sl_lookup_window(ctx, event->window);
    if (!window)
      return;

    window->dark_frame = 0;

    if (event->state != XCB_PROPERTY_DELETE) {
      xcb_get_property_reply_t* reply = xcb_get_property_reply(
          ctx->connection,
          xcb_get_property(ctx->connection, 0, window->id,
                           ctx->atoms[ATOM_GTK_THEME_VARIANT].value,
                           XCB_ATOM_ANY, 0, 2048),
          NULL);
      if (reply) {
        if (xcb_get_property_value_length(reply) >= 4)
          window->dark_frame = !strcmp(xcb_get_property_value(reply), "dark");
        free(reply);
      }
    }

    if (!window->aura_surface)
      return;

    frame_color = window->dark_frame ? ctx->dark_frame_color : ctx->frame_color;
    zaura_surface_set_frame_colors(window->aura_surface, frame_color,
                                   frame_color);
  } else if (event->atom == ctx->atoms[ATOM_WL_SELECTION].value) {
    if (event->window == ctx->selection_window &&
        event->state == XCB_PROPERTY_NEW_VALUE &&
        ctx->selection_incremental_transfer) {
      xcb_get_property_reply_t* reply = xcb_get_property_reply(
          ctx->connection,
          xcb_get_property(ctx->connection, 0, ctx->selection_window,
                           ctx->atoms[ATOM_WL_SELECTION].value,
                           XCB_GET_PROPERTY_TYPE_ANY, 0, 0x1fffffff),
          NULL);

      if (!reply)
        return;

      if (xcb_get_property_value_length(reply) > 0) {
        sl_write_selection_property(ctx, reply);
      } else {
        assert(!ctx->selection_send_event_source);
        close(ctx->selection_data_source_send_fd);
        ctx->selection_data_source_send_fd = -1;
        free(reply);

        sl_process_data_source_send_pending_list(ctx);
      }
    }
  } else if (event->atom == ctx->selection_request.property) {
    if (event->window == ctx->selection_request.requestor &&
        event->state == XCB_PROPERTY_DELETE &&
        ctx->selection_incremental_transfer) {
      int data_size = ctx->selection_data.size;

      ctx->selection_data_ack_pending = 0;

      // Handle the case when there's more data to be received.
      if (ctx->selection_data_offer_receive_fd >= 0) {
        // Avoid sending empty data until transfer is complete.
        if (data_size)
          sl_send_selection_data(ctx);

        if (!ctx->selection_event_source) {
          ctx->selection_event_source = wl_event_loop_add_fd(
              wl_display_get_event_loop(ctx->host_display),
              ctx->selection_data_offer_receive_fd, WL_EVENT_READABLE,
              sl_handle_selection_fd_readable, ctx);
        }
        return;
      }

      sl_send_selection_data(ctx);

      // Release data if transfer is complete.
      if (!data_size) {
        ctx->selection_request.requestor = XCB_NONE;
        wl_array_release(&ctx->selection_data);
      }
    }
  }
}

static void sl_internal_data_source_target(void* data,
                                           struct wl_data_source* data_source,
                                           const char* mime_type) {}

static void sl_internal_data_source_send(void* data,
                                         struct wl_data_source* data_source,
                                         const char* mime_type,
                                         int32_t fd) {
  struct sl_data_source* host = data;
  struct sl_context* ctx = host->ctx;

  xcb_intern_atom_cookie_t cookie =
      xcb_intern_atom(ctx->connection, false, strlen(mime_type), mime_type);

  if (ctx->selection_data_source_send_fd < 0) {
    sl_begin_data_source_send(ctx, fd, cookie, host);
  } else {
    struct sl_data_source_send_request* request =
        malloc(sizeof(struct sl_data_source_send_request));

    request->fd = fd;
    request->cookie = cookie;
    request->data_source = host;
    wl_list_insert(&ctx->selection_data_source_send_pending, &request->link);
  }
}

static void sl_internal_data_source_cancelled(
    void* data, struct wl_data_source* data_source) {
  struct sl_data_source* host = data;

  if (host->ctx->selection_data_source == host)
    host->ctx->selection_data_source = NULL;

  wl_data_source_destroy(data_source);
}

static const struct wl_data_source_listener sl_internal_data_source_listener = {
    sl_internal_data_source_target, sl_internal_data_source_send,
    sl_internal_data_source_cancelled};

char* sl_copy_atom_name(xcb_get_atom_name_reply_t* reply) {
  // The string produced by xcb_get_atom_name_name isn't null terminated, so we
  // have to copy |name_len| bytes into a new buffer and add the null character
  // ourselves.
  char* name_start = xcb_get_atom_name_name(reply);
  int name_len = xcb_get_atom_name_name_length(reply);
  char* name = malloc(name_len + 1);
  memcpy(name, name_start, name_len);
  name[name_len] = '\0';
  return name;
}

static void sl_get_selection_targets(struct sl_context* ctx) {
  struct sl_data_source* data_source = NULL;
  xcb_get_property_reply_t* reply;
  xcb_atom_t* value;
  uint32_t i;

  reply = xcb_get_property_reply(
      ctx->connection,
      xcb_get_property(ctx->connection, 1, ctx->selection_window,
                       ctx->atoms[ATOM_WL_SELECTION].value,
                       XCB_GET_PROPERTY_TYPE_ANY, 0, 4096),
      NULL);
  if (!reply)
    return;

  if (reply->type != XCB_ATOM_ATOM) {
    free(reply);
    return;
  }

  if (ctx->data_device_manager) {
    data_source = malloc(sizeof(*data_source));
    assert(data_source);

    data_source->ctx = ctx;
    data_source->internal = wl_data_device_manager_create_data_source(
        ctx->data_device_manager->internal);
    wl_data_source_add_listener(data_source->internal,
                                &sl_internal_data_source_listener, data_source);

    value = xcb_get_property_value(reply);

    // We need to convert all of the offered target types from X11 atoms to
    // strings (i.e. getting the names of the atoms). Each conversion requires a
    // round trip to the X server, but none of the requests depend on each
    // other. Therefore, we can speed things up by sending out all the requests
    // as a batch with xcb_get_atom_name, and then read all the replies as a
    // batch with xcb_get_atom_name_reply.
    xcb_get_atom_name_cookie_t* atom_name_cookies =
        malloc(sizeof(xcb_get_atom_name_cookie_t) * reply->value_len);
    for (i = 0; i < reply->value_len; i++) {
      atom_name_cookies[i] = xcb_get_atom_name(ctx->connection, value[i]);
    }
    for (i = 0; i < reply->value_len; i++) {
      xcb_get_atom_name_reply_t* atom_name_reply =
          xcb_get_atom_name_reply(ctx->connection, atom_name_cookies[i], NULL);
      if (atom_name_reply) {
        char* name = sl_copy_atom_name(atom_name_reply);
        wl_data_source_offer(data_source->internal, name);
        free(atom_name_reply);
        free(name);
      }
    }
    free(atom_name_cookies);

    if (ctx->selection_data_device && ctx->default_seat) {
      wl_data_device_set_selection(ctx->selection_data_device,
                                   data_source->internal,
                                   ctx->default_seat->seat->last_serial);
    }

    if (ctx->selection_data_source) {
      wl_data_source_destroy(ctx->selection_data_source->internal);
      free(ctx->selection_data_source);
    }
    ctx->selection_data_source = data_source;
  }

  free(reply);
}

static void sl_get_selection_data(struct sl_context* ctx) {
  xcb_get_property_reply_t* reply = xcb_get_property_reply(
      ctx->connection,
      xcb_get_property(ctx->connection, 1, ctx->selection_window,
                       ctx->atoms[ATOM_WL_SELECTION].value,
                       XCB_GET_PROPERTY_TYPE_ANY, 0, 0x1fffffff),
      NULL);
  if (!reply)
    return;

  if (reply->type == ctx->atoms[ATOM_INCR].value) {
    ctx->selection_incremental_transfer = 1;
    free(reply);
  } else {
    ctx->selection_incremental_transfer = 0;
    sl_write_selection_property(ctx, reply);
  }
}

static void sl_handle_selection_notify(struct sl_context* ctx,
                                       xcb_selection_notify_event_t* event) {
  if (event->property == XCB_ATOM_NONE)
    return;

  if (event->target == ctx->atoms[ATOM_TARGETS].value)
    sl_get_selection_targets(ctx);
  else
    sl_get_selection_data(ctx);
}

static void sl_send_targets(struct sl_context* ctx) {
  xcb_change_property(
      ctx->connection, XCB_PROP_MODE_REPLACE, ctx->selection_request.requestor,
      ctx->selection_request.property, XCB_ATOM_ATOM, 32,
      ctx->selection_data_offer->atoms.size / sizeof(xcb_atom_t),
      ctx->selection_data_offer->atoms.data);

  sl_send_selection_notify(ctx, ctx->selection_request.property);
}

static void sl_send_timestamp(struct sl_context* ctx) {
  xcb_change_property(ctx->connection, XCB_PROP_MODE_REPLACE,
                      ctx->selection_request.requestor,
                      ctx->selection_request.property, XCB_ATOM_INTEGER, 32, 1,
                      &ctx->selection_timestamp);

  sl_send_selection_notify(ctx, ctx->selection_request.property);
}

static void sl_send_data(struct sl_context* ctx, xcb_atom_t data_type) {
  int rv, fd_to_receive, fd_to_wayland;

  if (!ctx->selection_data_offer) {
    sl_send_selection_notify(ctx, XCB_ATOM_NONE);
    return;
  }

  if (ctx->selection_event_source) {
    fprintf(stderr, "error: selection transfer already pending\n");
    sl_send_selection_notify(ctx, XCB_ATOM_NONE);
    return;
  }

  ctx->selection_data_type = data_type;

  // We will need the name of this atom later to tell the wayland server what
  // type of data to send us, so start the request now.
  xcb_get_atom_name_cookie_t atom_name_cookie =
      xcb_get_atom_name(ctx->connection, data_type);

  wl_array_init(&ctx->selection_data);
  ctx->selection_data_ack_pending = 0;

  switch (ctx->data_driver) {
    case DATA_DRIVER_VIRTWL: {
      struct virtwl_ioctl_new new_pipe = {
          .type = VIRTWL_IOCTL_NEW_PIPE_READ,
          .fd = -1,
          .flags = 0,
          .size = 0,
      };

      rv = ioctl(ctx->virtwl_fd, VIRTWL_IOCTL_NEW, &new_pipe);
      if (rv) {
        fprintf(stderr, "error: failed to create virtwl pipe: %s\n",
                strerror(errno));
        sl_send_selection_notify(ctx, XCB_ATOM_NONE);
        return;
      }

      fd_to_receive = new_pipe.fd;
      fd_to_wayland = new_pipe.fd;

    } break;
    case DATA_DRIVER_NOOP: {
      int p[2];

      rv = pipe2(p, O_CLOEXEC | O_NONBLOCK);
      assert(!rv);

      fd_to_receive = p[0];
      fd_to_wayland = p[1];

    } break;
  }

  xcb_get_atom_name_reply_t* atom_name_reply =
      xcb_get_atom_name_reply(ctx->connection, atom_name_cookie, NULL);
  if (atom_name_reply) {
    // If we got the atom name, then send the request to wayland and add our end
    // of the pipe to the wayland event loop.
    ctx->selection_data_offer_receive_fd = fd_to_receive;
    char* name = sl_copy_atom_name(atom_name_reply);
    wl_data_offer_receive(ctx->selection_data_offer->internal, name,
                          fd_to_wayland);
    free(atom_name_reply);
    free(name);

    ctx->selection_event_source = wl_event_loop_add_fd(
        wl_display_get_event_loop(ctx->host_display),
        ctx->selection_data_offer_receive_fd, WL_EVENT_READABLE,
        sl_handle_selection_fd_readable, ctx);
  } else {
    // If getting the atom name failed, notify the requestor that there won't be
    // any data, and close our end of the pipe.
    close(fd_to_receive);
    sl_send_selection_notify(ctx, XCB_ATOM_NONE);
  }

  // Close the wayland end of the pipe, now that it's either been sent or not
  // going to be sent. The VIRTWL driver uses the same fd for both ends of the
  // pipe, so don't close the fd if both ends are the same.
  if (fd_to_receive != fd_to_wayland)
    close(fd_to_wayland);
}

static void sl_handle_selection_request(struct sl_context* ctx,
                                        xcb_selection_request_event_t* event) {
  ctx->selection_request = *event;
  ctx->selection_incremental_transfer = 0;

  if (event->selection == ctx->atoms[ATOM_CLIPBOARD_MANAGER].value) {
    sl_send_selection_notify(ctx, ctx->selection_request.property);
    return;
  }

  if (event->target == ctx->atoms[ATOM_TARGETS].value) {
    sl_send_targets(ctx);
  } else if (event->target == ctx->atoms[ATOM_TIMESTAMP].value) {
    sl_send_timestamp(ctx);
  } else {
    int success = 0;
    xcb_atom_t* atom;
    wl_array_for_each(atom, &ctx->selection_data_offer->atoms) {
      if (event->target == *atom) {
        success = 1;
        sl_send_data(ctx, *atom);
        break;
      }
    }
    if (!success) {
      sl_send_selection_notify(ctx, XCB_ATOM_NONE);
    }
  }
}

static void sl_handle_xfixes_selection_notify(
    struct sl_context* ctx, xcb_xfixes_selection_notify_event_t* event) {
  if (event->selection != ctx->atoms[ATOM_CLIPBOARD].value)
    return;

  if (event->owner == XCB_WINDOW_NONE) {
    // If client selection is gone. Set NULL selection for each seat.
    if (ctx->selection_owner != ctx->selection_window) {
      if (ctx->selection_data_device && ctx->default_seat) {
        wl_data_device_set_selection(ctx->selection_data_device, NULL,
                                     ctx->default_seat->seat->last_serial);
      }
    }
    ctx->selection_owner = XCB_WINDOW_NONE;
    return;
  }

  ctx->selection_owner = event->owner;

  // Save timestamp if it's our selection.
  if (event->owner == ctx->selection_window) {
    ctx->selection_timestamp = event->timestamp;
    return;
  }

  ctx->selection_incremental_transfer = 0;
  xcb_convert_selection(ctx->connection, ctx->selection_window,
                        ctx->atoms[ATOM_CLIPBOARD].value,
                        ctx->atoms[ATOM_TARGETS].value,
                        ctx->atoms[ATOM_WL_SELECTION].value, event->timestamp);
}

static int sl_handle_x_connection_event(int fd, uint32_t mask, void* data) {
  struct sl_context* ctx = (struct sl_context*)data;
  xcb_generic_event_t* event;
  uint32_t count = 0;

  if ((mask & WL_EVENT_HANGUP) || (mask & WL_EVENT_ERROR))
    return 0;

  while ((event = xcb_poll_for_event(ctx->connection))) {
    switch (event->response_type & ~SEND_EVENT_MASK) {
      case XCB_CREATE_NOTIFY:
        sl_handle_create_notify(ctx, (xcb_create_notify_event_t*)event);
        break;
      case XCB_DESTROY_NOTIFY:
        sl_handle_destroy_notify(ctx, (xcb_destroy_notify_event_t*)event);
        break;
      case XCB_REPARENT_NOTIFY:
        sl_handle_reparent_notify(ctx, (xcb_reparent_notify_event_t*)event);
        break;
      case XCB_MAP_REQUEST:
        sl_handle_map_request(ctx, (xcb_map_request_event_t*)event);
        break;
      case XCB_MAP_NOTIFY:
        sl_handle_map_notify(ctx, (xcb_map_notify_event_t*)event);
        break;
      case XCB_UNMAP_NOTIFY:
        sl_handle_unmap_notify(ctx, (xcb_unmap_notify_event_t*)event);
        break;
      case XCB_CONFIGURE_REQUEST:
        sl_handle_configure_request(ctx, (xcb_configure_request_event_t*)event);
        break;
      case XCB_CONFIGURE_NOTIFY:
        sl_handle_configure_notify(ctx, (xcb_configure_notify_event_t*)event);
        break;
      case XCB_CLIENT_MESSAGE:
        sl_handle_client_message(ctx, (xcb_client_message_event_t*)event);
        break;
      case XCB_FOCUS_IN:
        sl_handle_focus_in(ctx, (xcb_focus_in_event_t*)event);
        break;
      case XCB_FOCUS_OUT:
        sl_handle_focus_out(ctx, (xcb_focus_out_event_t*)event);
        break;
      case XCB_PROPERTY_NOTIFY:
        sl_handle_property_notify(ctx, (xcb_property_notify_event_t*)event);
        break;
      case XCB_SELECTION_NOTIFY:
        sl_handle_selection_notify(ctx, (xcb_selection_notify_event_t*)event);
        break;
      case XCB_SELECTION_REQUEST:
        sl_handle_selection_request(ctx, (xcb_selection_request_event_t*)event);
        break;
    }

    switch (event->response_type - ctx->xfixes_extension->first_event) {
      case XCB_XFIXES_SELECTION_NOTIFY:
        sl_handle_xfixes_selection_notify(
            ctx, (xcb_xfixes_selection_notify_event_t*)event);
        break;
    }

    free(event);
    ++count;
  }

  if ((mask & ~WL_EVENT_WRITABLE) == 0)
    xcb_flush(ctx->connection);

  return count;
}

static void sl_connect(struct sl_context* ctx) {
  const char wm_name[] = "Sommelier";
  const xcb_setup_t* setup;
  xcb_screen_iterator_t screen_iterator;
  uint32_t values[1];
  xcb_void_cookie_t change_attributes_cookie, redirect_subwindows_cookie;
  xcb_generic_error_t* error;
  xcb_intern_atom_reply_t* atom_reply;
  xcb_depth_iterator_t depth_iterator;
  xcb_xfixes_query_version_reply_t* xfixes_query_version_reply;
  const xcb_query_extension_reply_t* composite_extension;
  unsigned i;

  ctx->connection = xcb_connect_to_fd(ctx->wm_fd, NULL);
  assert(!xcb_connection_has_error(ctx->connection));

  xcb_prefetch_extension_data(ctx->connection, &xcb_xfixes_id);
  xcb_prefetch_extension_data(ctx->connection, &xcb_composite_id);

  for (i = 0; i < ARRAY_SIZE(ctx->atoms); ++i) {
    const char* name = ctx->atoms[i].name;
    ctx->atoms[i].cookie =
        xcb_intern_atom(ctx->connection, 0, strlen(name), name);
  }

  setup = xcb_get_setup(ctx->connection);
  screen_iterator = xcb_setup_roots_iterator(setup);
  ctx->screen = screen_iterator.data;

  // Select for substructure redirect.
  values[0] = XCB_EVENT_MASK_STRUCTURE_NOTIFY |
              XCB_EVENT_MASK_SUBSTRUCTURE_NOTIFY |
              XCB_EVENT_MASK_SUBSTRUCTURE_REDIRECT;
  change_attributes_cookie = xcb_change_window_attributes(
      ctx->connection, ctx->screen->root, XCB_CW_EVENT_MASK, values);

  ctx->connection_event_source = wl_event_loop_add_fd(
      wl_display_get_event_loop(ctx->host_display),
      xcb_get_file_descriptor(ctx->connection), WL_EVENT_READABLE,
      &sl_handle_x_connection_event, ctx);

  ctx->xfixes_extension =
      xcb_get_extension_data(ctx->connection, &xcb_xfixes_id);
  assert(ctx->xfixes_extension->present);

  xfixes_query_version_reply = xcb_xfixes_query_version_reply(
      ctx->connection,
      xcb_xfixes_query_version(ctx->connection, XCB_XFIXES_MAJOR_VERSION,
                               XCB_XFIXES_MINOR_VERSION),
      NULL);
  assert(xfixes_query_version_reply);
  assert(xfixes_query_version_reply->major_version >= 5);
  free(xfixes_query_version_reply);

  composite_extension =
      xcb_get_extension_data(ctx->connection, &xcb_composite_id);
  assert(composite_extension->present);
  UNUSED(composite_extension);

  redirect_subwindows_cookie = xcb_composite_redirect_subwindows_checked(
      ctx->connection, ctx->screen->root, XCB_COMPOSITE_REDIRECT_MANUAL);

  // Another window manager should not be running.
  error = xcb_request_check(ctx->connection, change_attributes_cookie);
  assert(!error);

  // Redirecting subwindows of root for compositing should have succeeded.
  error = xcb_request_check(ctx->connection, redirect_subwindows_cookie);
  assert(!error);

  ctx->window = xcb_generate_id(ctx->connection);
  xcb_create_window(ctx->connection, 0, ctx->window, ctx->screen->root, 0, 0, 1,
                    1, 0, XCB_WINDOW_CLASS_INPUT_ONLY, XCB_COPY_FROM_PARENT, 0,
                    NULL);

  for (i = 0; i < ARRAY_SIZE(ctx->atoms); ++i) {
    atom_reply =
        xcb_intern_atom_reply(ctx->connection, ctx->atoms[i].cookie, &error);
    assert(!error);
    ctx->atoms[i].value = atom_reply->atom;
    free(atom_reply);
  }

  depth_iterator = xcb_screen_allowed_depths_iterator(ctx->screen);
  while (depth_iterator.rem > 0) {
    int depth = depth_iterator.data->depth;
    if (depth == ctx->screen->root_depth) {
      ctx->visual_ids[depth] = ctx->screen->root_visual;
      ctx->colormaps[depth] = ctx->screen->default_colormap;
    } else {
      xcb_visualtype_iterator_t visualtype_iterator =
          xcb_depth_visuals_iterator(depth_iterator.data);

      ctx->visual_ids[depth] = visualtype_iterator.data->visual_id;
      ctx->colormaps[depth] = xcb_generate_id(ctx->connection);
      xcb_create_colormap(ctx->connection, XCB_COLORMAP_ALLOC_NONE,
                          ctx->colormaps[depth], ctx->screen->root,
                          ctx->visual_ids[depth]);
    }
    xcb_depth_next(&depth_iterator);
  }
  assert(ctx->visual_ids[ctx->screen->root_depth]);

  if (ctx->clipboard_manager) {
    values[0] = XCB_EVENT_MASK_PROPERTY_CHANGE;
    ctx->selection_window = xcb_generate_id(ctx->connection);
    xcb_create_window(ctx->connection, XCB_COPY_FROM_PARENT,
                      ctx->selection_window, ctx->screen->root, 0, 0, 1, 1, 0,
                      XCB_WINDOW_CLASS_INPUT_OUTPUT, ctx->screen->root_visual,
                      XCB_CW_EVENT_MASK, values);
    xcb_set_selection_owner(ctx->connection, ctx->selection_window,
                            ctx->atoms[ATOM_CLIPBOARD_MANAGER].value,
                            XCB_CURRENT_TIME);
    xcb_xfixes_select_selection_input(
        ctx->connection, ctx->selection_window,
        ctx->atoms[ATOM_CLIPBOARD].value,
        XCB_XFIXES_SELECTION_EVENT_MASK_SET_SELECTION_OWNER |
            XCB_XFIXES_SELECTION_EVENT_MASK_SELECTION_WINDOW_DESTROY |
            XCB_XFIXES_SELECTION_EVENT_MASK_SELECTION_CLIENT_CLOSE);
    sl_set_selection(ctx, NULL);
  }

  xcb_change_property(ctx->connection, XCB_PROP_MODE_REPLACE, ctx->window,
                      ctx->atoms[ATOM_NET_SUPPORTING_WM_CHECK].value,
                      XCB_ATOM_WINDOW, 32, 1, &ctx->window);
  xcb_change_property(ctx->connection, XCB_PROP_MODE_REPLACE, ctx->window,
                      ctx->atoms[ATOM_NET_WM_NAME].value,
                      ctx->atoms[ATOM_UTF8_STRING].value, 8, strlen(wm_name),
                      wm_name);
  xcb_change_property(ctx->connection, XCB_PROP_MODE_REPLACE, ctx->screen->root,
                      ctx->atoms[ATOM_NET_SUPPORTING_WM_CHECK].value,
                      XCB_ATOM_WINDOW, 32, 1, &ctx->window);
  xcb_set_selection_owner(ctx->connection, ctx->window,
                          ctx->atoms[ATOM_WM_S0].value, XCB_CURRENT_TIME);

  xcb_set_input_focus(ctx->connection, XCB_INPUT_FOCUS_NONE, XCB_NONE,
                      XCB_CURRENT_TIME);
  xcb_flush(ctx->connection);
}

static void sl_sd_notify(const char* state) {
  const char* socket_name;
  struct msghdr msghdr;
  struct iovec iovec;
  struct sockaddr_un addr;
  int fd;
  int rv;

  socket_name = getenv("NOTIFY_SOCKET");
  assert(socket_name);

  fd = socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0);
  assert(fd >= 0);

  memset(&addr, 0, sizeof(addr));
  addr.sun_family = AF_UNIX;
  strncpy(addr.sun_path, socket_name, sizeof(addr.sun_path));

  memset(&iovec, 0, sizeof(iovec));
  iovec.iov_base = (char*)state;
  iovec.iov_len = strlen(state);

  memset(&msghdr, 0, sizeof(msghdr));
  msghdr.msg_name = &addr;
  msghdr.msg_namelen =
      offsetof(struct sockaddr_un, sun_path) + strlen(socket_name);
  msghdr.msg_iov = &iovec;
  msghdr.msg_iovlen = 1;

  rv = sendmsg(fd, &msghdr, MSG_NOSIGNAL);
  assert(rv != -1);
  UNUSED(rv);
}

static int sl_handle_sigchld(int signal_number, void* data) {
  struct sl_context* ctx = (struct sl_context*)data;
  int status;
  pid_t pid;

  while ((pid = waitpid(-1, &status, WNOHANG)) > 0) {
    if (pid == ctx->child_pid) {
      ctx->child_pid = -1;
      if (WIFEXITED(status) && WEXITSTATUS(status)) {
        fprintf(stderr, "Child exited with status: %d\n", WEXITSTATUS(status));
      }
      if (ctx->exit_with_child) {
        if (ctx->xwayland_pid >= 0)
          kill(ctx->xwayland_pid, SIGTERM);
      } else {
        // Notify systemd that we are ready to accept connections now that
        // child process has finished running and all environment is ready.
        if (ctx->sd_notify)
          sl_sd_notify(ctx->sd_notify);
      }
    } else if (pid == ctx->xwayland_pid) {
      ctx->xwayland_pid = -1;
      if (WIFEXITED(status) && WEXITSTATUS(status)) {
        fprintf(stderr, "Xwayland exited with status: %d\n",
                WEXITSTATUS(status));
        exit(WEXITSTATUS(status));
      }
    }
  }

  return 1;
}

static void sl_execvp(const char* file,
                      char* const argv[],
                      int wayland_socked_fd) {
  if (wayland_socked_fd >= 0) {
    int fd;
    fd = dup(wayland_socked_fd);
    putenv(sl_xasprintf("WAYLAND_SOCKET=%d", fd));
  }

  setenv("SOMMELIER_VERSION", SOMMELIER_VERSION, 1);

  execvp(file, argv);
  perror(file);
}

static void sl_calculate_scale_for_xwayland(struct sl_context* ctx) {
  struct sl_host_output* output;
  double default_scale_factor = 1.0;
  double scale;

  // Find internal output and determine preferred scale factor.
  wl_list_for_each(output, &ctx->host_outputs, link) {
    if (output->internal) {
      double preferred_scale =
          sl_output_aura_scale_factor_to_double(output->preferred_scale);

      if (ctx->aura_shell) {
        double device_scale_factor =
            sl_output_aura_scale_factor_to_double(output->device_scale_factor);

        default_scale_factor = device_scale_factor * preferred_scale;
      }
      break;
    }
  }

  // We use the default scale factor multipled by desired scale set by the
  // user. This gives us HiDPI support by default but the user can still
  // adjust it if higher or lower density is preferred.
  scale = ctx->desired_scale * default_scale_factor;

  // Round to integer scale if wp_viewporter interface is not present.
  if (!ctx->viewporter)
    scale = round(scale);

  // Clamp and set scale.
  ctx->scale = MIN(MAX_SCALE, MAX(MIN_SCALE, scale));

  // Scale affects output state. Send updated output state to xwayland.
  wl_list_for_each(output, &ctx->host_outputs, link)
      sl_output_send_host_output_state(output);
}

static int sl_handle_display_ready_event(int fd, uint32_t mask, void* data) {
  struct sl_context* ctx = (struct sl_context*)data;
  char display_name[9];
  int bytes_read = 0;
  pid_t pid;

  if (!(mask & WL_EVENT_READABLE))
    return 0;

  display_name[0] = ':';
  do {
    int bytes_left = sizeof(display_name) - bytes_read - 1;
    int bytes;

    if (!bytes_left)
      break;

    bytes = read(fd, &display_name[bytes_read + 1], bytes_left);
    if (!bytes)
      break;

    bytes_read += bytes;
  } while (display_name[bytes_read] != '\n');

  display_name[bytes_read] = '\0';
  setenv("DISPLAY", display_name, 1);

  sl_connect(ctx);

  wl_event_source_remove(ctx->display_ready_event_source);
  ctx->display_ready_event_source = NULL;
  close(fd);

  // Calculate scale now that the default scale factor is known. This also
  // happens to workaround an issue in Xwayland where an output update is
  // needed for DPI to be set correctly.
  sl_calculate_scale_for_xwayland(ctx);
  wl_display_flush_clients(ctx->host_display);

  putenv(sl_xasprintf("XCURSOR_SIZE=%d",
                      (int)(XCURSOR_SIZE_BASE * ctx->scale + 0.5)));

  pid = fork();
  assert(pid >= 0);
  if (pid == 0) {
    sl_execvp(ctx->runprog[0], ctx->runprog, -1);
    _exit(EXIT_FAILURE);
  }

  ctx->child_pid = pid;

  return 1;
}

static void sl_sigchld_handler(int signal) {
  while (waitpid(-1, NULL, WNOHANG) > 0)
    continue;
}

static void sl_client_destroy_notify(struct wl_listener* listener, void* data) {
  exit(0);
}

static int sl_handle_virtwl_ctx_event(int fd, uint32_t mask, void* data) {
  struct sl_context* ctx = (struct sl_context*)data;
  uint8_t ioctl_buffer[4096];
  struct virtwl_ioctl_txn* ioctl_recv = (struct virtwl_ioctl_txn*)ioctl_buffer;
  void* recv_data = ioctl_buffer + sizeof(struct virtwl_ioctl_txn);
  size_t max_recv_size = sizeof(ioctl_buffer) - sizeof(struct virtwl_ioctl_txn);
  char fd_buffer[CMSG_LEN(sizeof(int) * VIRTWL_SEND_MAX_ALLOCS)];
  struct msghdr msg = {0};
  struct iovec buffer_iov;
  ssize_t bytes;
  int fd_count;
  int rv;

  ioctl_recv->len = max_recv_size;
  rv = ioctl(fd, VIRTWL_IOCTL_RECV, ioctl_recv);
  if (rv) {
    close(ctx->virtwl_socket_fd);
    ctx->virtwl_socket_fd = -1;
    return 0;
  }

  buffer_iov.iov_base = recv_data;
  buffer_iov.iov_len = ioctl_recv->len;

  msg.msg_iov = &buffer_iov;
  msg.msg_iovlen = 1;
  msg.msg_control = fd_buffer;

  // Count how many FDs the kernel gave us.
  for (fd_count = 0; fd_count < VIRTWL_SEND_MAX_ALLOCS; fd_count++) {
    if (ioctl_recv->fds[fd_count] < 0)
      break;
  }
  if (fd_count) {
    struct cmsghdr* cmsg;

    // Need to set msg_controllen so CMSG_FIRSTHDR will return the first
    // cmsghdr. We copy every fd we just received from the ioctl into this
    // cmsghdr.
    msg.msg_controllen = sizeof(fd_buffer);
    cmsg = CMSG_FIRSTHDR(&msg);
    cmsg->cmsg_level = SOL_SOCKET;
    cmsg->cmsg_type = SCM_RIGHTS;
    cmsg->cmsg_len = CMSG_LEN(fd_count * sizeof(int));
    memcpy(CMSG_DATA(cmsg), ioctl_recv->fds, fd_count * sizeof(int));
    msg.msg_controllen = cmsg->cmsg_len;
  }

  bytes = sendmsg(ctx->virtwl_socket_fd, &msg, MSG_NOSIGNAL);
  assert(bytes == ioctl_recv->len);
  UNUSED(bytes);

  while (fd_count--)
    close(ioctl_recv->fds[fd_count]);

  return 1;
}

static int sl_handle_virtwl_socket_event(int fd, uint32_t mask, void* data) {
  struct sl_context* ctx = (struct sl_context*)data;
  uint8_t ioctl_buffer[4096];
  struct virtwl_ioctl_txn* ioctl_send = (struct virtwl_ioctl_txn*)ioctl_buffer;
  void* send_data = ioctl_buffer + sizeof(struct virtwl_ioctl_txn);
  size_t max_send_size = sizeof(ioctl_buffer) - sizeof(struct virtwl_ioctl_txn);
  char fd_buffer[CMSG_LEN(sizeof(int) * VIRTWL_SEND_MAX_ALLOCS)];
  struct iovec buffer_iov;
  struct msghdr msg = {0};
  struct cmsghdr* cmsg;
  ssize_t bytes;
  int fd_count = 0;
  int rv;
  int i;

  buffer_iov.iov_base = send_data;
  buffer_iov.iov_len = max_send_size;

  msg.msg_iov = &buffer_iov;
  msg.msg_iovlen = 1;
  msg.msg_control = fd_buffer;
  msg.msg_controllen = sizeof(fd_buffer);

  bytes = recvmsg(ctx->virtwl_socket_fd, &msg, 0);
  assert(bytes > 0);

  // If there were any FDs recv'd by recvmsg, there will be some data in the
  // msg_control buffer. To get the FDs out we iterate all cmsghdr's within and
  // unpack the FDs if the cmsghdr type is SCM_RIGHTS.
  for (cmsg = msg.msg_controllen != 0 ? CMSG_FIRSTHDR(&msg) : NULL; cmsg;
       cmsg = CMSG_NXTHDR(&msg, cmsg)) {
    size_t cmsg_fd_count;

    if (cmsg->cmsg_level != SOL_SOCKET || cmsg->cmsg_type != SCM_RIGHTS)
      continue;

    cmsg_fd_count = (cmsg->cmsg_len - CMSG_LEN(0)) / sizeof(int);

    // fd_count will never exceed VIRTWL_SEND_MAX_ALLOCS because the
    // control message buffer only allocates enough space for that many FDs.
    memcpy(&ioctl_send->fds[fd_count], CMSG_DATA(cmsg),
           cmsg_fd_count * sizeof(int));
    fd_count += cmsg_fd_count;
  }

  for (i = fd_count; i < VIRTWL_SEND_MAX_ALLOCS; ++i)
    ioctl_send->fds[i] = -1;

  // The FDs and data were extracted from the recvmsg call into the ioctl_send
  // structure which we now pass along to the kernel.
  ioctl_send->len = bytes;
  rv = ioctl(ctx->virtwl_ctx_fd, VIRTWL_IOCTL_SEND, ioctl_send);
  assert(!rv);
  UNUSED(rv);

  while (fd_count--)
    close(ioctl_send->fds[fd_count]);

  return 1;
}

// Break |str| into a sequence of zero or more nonempty arguments. No more
// than |argc| arguments will be added to |argv|. Returns the total number of
// argments found in |str|.
static int sl_parse_cmd_prefix(char* str, int argc, char** argv) {
  char* s = str;
  int n = 0;
  int delim = 0;

  do {
    // Look for ending delimiter if |delim| is set.
    if (delim) {
      if (*s == delim) {
        delim = 0;
        *s = '\0';
      }
      ++s;
    } else {
      // Skip forward to first non-space character.
      while (*s == ' ' && *s != '\0')
        ++s;

      // Check for quote delimiter.
      if (*s == '"') {
        delim = '"';
        ++s;
      } else {
        delim = ' ';
      }

      // Add string to arguments if there's room.
      if (n < argc)
        argv[n] = s;

      ++n;
    }
  } while (*s != '\0');

  return n;
}

static void sl_print_usage() {
  printf(
      "usage: sommelier [options] [program] [args...]\n\n"
      "options:\n"
      "  -h, --help\t\t\tPrint this help\n"
      "  -X\t\t\t\tEnable X11 forwarding\n"
      "  --master\t\t\tRun as master and spawn child processes\n"
      "  --socket=SOCKET\t\tName of socket to listen on\n"
      "  --display=DISPLAY\t\tWayland display to connect to\n"
      "  --shm-driver=DRIVER\t\tSHM driver to use (noop, dmabuf, virtwl)\n"
      "  --data-driver=DRIVER\t\tData driver to use (noop, virtwl)\n"
      "  --scale=SCALE\t\t\tScale factor for contents\n"
      "  --dpi=[DPI[,DPI...]]\t\tDPI buckets\n"
      "  --peer-cmd-prefix=PREFIX\tPeer process command line prefix\n"
      "  --accelerators=ACCELERATORS\tList of keyboard accelerators\n"
      "  --application-id=ID\t\tForced application ID for X11 clients\n"
      "  --x-display=DISPLAY\t\tX11 display to listen on\n"
      "  --xwayland-path=PATH\t\tPath to Xwayland executable\n"
      "  --xwayland-gl-driver-path=PATH\tPath to GL drivers for Xwayland\n"
      "  --xwayland-cmd-prefix=PREFIX\tXwayland command line prefix\n"
      "  --no-exit-with-child\t\tKeep process alive after child exists\n"
      "  --no-clipboard-manager\tDisable X11 clipboard manager\n"
      "  --frame-color=COLOR\t\tWindow frame color for X11 clients\n"
      "  --virtwl-device=DEVICE\tVirtWL device to use\n"
      "  --drm-device=DEVICE\t\tDRM device to use\n"
      "  --glamor\t\t\tUse glamor to accelerate X11 clients\n");
}

static const char* sl_arg_value(const char* arg) {
  const char* s = strchr(arg, '=');
  if (!s) {
    sl_print_usage();
    exit(EXIT_FAILURE);
  }
  return s + 1;
}

int main(int argc, char** argv) {
  struct sl_context ctx = {
      .runprog = NULL,
      .display = NULL,
      .host_display = NULL,
      .client = NULL,
      .compositor = NULL,
      .subcompositor = NULL,
      .shm = NULL,
      .shell = NULL,
      .data_device_manager = NULL,
      .xdg_shell = NULL,
      .aura_shell = NULL,
      .viewporter = NULL,
      .linux_dmabuf = NULL,
      .keyboard_extension = NULL,
      .text_input_manager = NULL,
      .display_event_source = NULL,
      .display_ready_event_source = NULL,
      .sigchld_event_source = NULL,
      .shm_driver = SHM_DRIVER_NOOP,
      .data_driver = DATA_DRIVER_NOOP,
      .wm_fd = -1,
      .virtwl_fd = -1,
      .virtwl_ctx_fd = -1,
      .virtwl_socket_fd = -1,
      .virtwl_ctx_event_source = NULL,
      .virtwl_socket_event_source = NULL,
      .drm_device = NULL,
      .gbm = NULL,
      .xwayland = 0,
      .xwayland_pid = -1,
      .child_pid = -1,
      .peer_pid = -1,
      .xkb_context = NULL,
      .next_global_id = 1,
      .connection = NULL,
      .connection_event_source = NULL,
      .xfixes_extension = NULL,
      .screen = NULL,
      .window = 0,
      .host_focus_window = NULL,
      .needs_set_input_focus = 0,
      .desired_scale = 1.0,
      .scale = 1.0,
      .application_id = NULL,
      .exit_with_child = 1,
      .sd_notify = NULL,
      .clipboard_manager = 0,
      .frame_color = 0xffffffff,
      .dark_frame_color = 0xff000000,
      .default_seat = NULL,
      .selection_window = XCB_WINDOW_NONE,
      .selection_owner = XCB_WINDOW_NONE,
      .selection_incremental_transfer = 0,
      .selection_request = {.requestor = XCB_NONE, .property = XCB_ATOM_NONE},
      .selection_timestamp = XCB_CURRENT_TIME,
      .selection_data_device = NULL,
      .selection_data_offer = NULL,
      .selection_data_source = NULL,
      .selection_data_source_send_fd = -1,
      .selection_send_event_source = NULL,
      .selection_property_reply = NULL,
      .selection_property_offset = 0,
      .selection_event_source = NULL,
      .selection_data_offer_receive_fd = -1,
      .selection_data_ack_pending = 0,
      .atoms =
          {
              [ATOM_WM_S0] = {"WM_S0"},
              [ATOM_WM_PROTOCOLS] = {"WM_PROTOCOLS"},
              [ATOM_WM_STATE] = {"WM_STATE"},
              [ATOM_WM_CHANGE_STATE] = {"WM_CHANGE_STATE"},
              [ATOM_WM_DELETE_WINDOW] = {"WM_DELETE_WINDOW"},
              [ATOM_WM_TAKE_FOCUS] = {"WM_TAKE_FOCUS"},
              [ATOM_WM_CLIENT_LEADER] = {"WM_CLIENT_LEADER"},
              [ATOM_WL_SURFACE_ID] = {"WL_SURFACE_ID"},
              [ATOM_UTF8_STRING] = {"UTF8_STRING"},
              [ATOM_MOTIF_WM_HINTS] = {"_MOTIF_WM_HINTS"},
              [ATOM_NET_FRAME_EXTENTS] = {"_NET_FRAME_EXTENTS"},
              [ATOM_NET_STARTUP_ID] = {"_NET_STARTUP_ID"},
              [ATOM_NET_SUPPORTING_WM_CHECK] = {"_NET_SUPPORTING_WM_CHECK"},
              [ATOM_NET_WM_NAME] = {"_NET_WM_NAME"},
              [ATOM_NET_WM_MOVERESIZE] = {"_NET_WM_MOVERESIZE"},
              [ATOM_NET_WM_STATE] = {"_NET_WM_STATE"},
              [ATOM_NET_WM_STATE_FULLSCREEN] = {"_NET_WM_STATE_FULLSCREEN"},
              [ATOM_NET_WM_STATE_MAXIMIZED_VERT] =
                  {"_NET_WM_STATE_MAXIMIZED_VERT"},
              [ATOM_NET_WM_STATE_MAXIMIZED_HORZ] =
                  {"_NET_WM_STATE_MAXIMIZED_HORZ"},
              [ATOM_CLIPBOARD] = {"CLIPBOARD"},
              [ATOM_CLIPBOARD_MANAGER] = {"CLIPBOARD_MANAGER"},
              [ATOM_TARGETS] = {"TARGETS"},
              [ATOM_TIMESTAMP] = {"TIMESTAMP"},
              [ATOM_TEXT] = {"TEXT"},
              [ATOM_INCR] = {"INCR"},
              [ATOM_WL_SELECTION] = {"_WL_SELECTION"},
              [ATOM_GTK_THEME_VARIANT] = {"_GTK_THEME_VARIANT"},
          },
      .visual_ids = {0},
      .colormaps = {0}};
  const char* display = getenv("SOMMELIER_DISPLAY");
  const char* scale = getenv("SOMMELIER_SCALE");
  const char* dpi = getenv("SOMMELIER_DPI");
  const char* clipboard_manager = getenv("SOMMELIER_CLIPBOARD_MANAGER");
  const char* frame_color = getenv("SOMMELIER_FRAME_COLOR");
  const char* dark_frame_color = getenv("SOMMELIER_DARK_FRAME_COLOR");
  const char* virtwl_device = getenv("SOMMELIER_VIRTWL_DEVICE");
  const char* drm_device = getenv("SOMMELIER_DRM_DEVICE");
  const char* glamor = getenv("SOMMELIER_GLAMOR");
  const char* shm_driver = getenv("SOMMELIER_SHM_DRIVER");
  const char* data_driver = getenv("SOMMELIER_DATA_DRIVER");
  const char* peer_cmd_prefix = getenv("SOMMELIER_PEER_CMD_PREFIX");
  const char* xwayland_cmd_prefix = getenv("SOMMELIER_XWAYLAND_CMD_PREFIX");
  const char* accelerators = getenv("SOMMELIER_ACCELERATORS");
  const char* xwayland_path = getenv("SOMMELIER_XWAYLAND_PATH");
  const char* xwayland_gl_driver_path =
      getenv("SOMMELIER_XWAYLAND_GL_DRIVER_PATH");
  const char* xauth_path = getenv("SOMMELIER_XAUTH_PATH");
  const char* xfont_path = getenv("SOMMELIER_XFONT_PATH");
  const char* socket_name = "wayland-0";
  const char* runtime_dir;
  struct wl_event_loop* event_loop;
  struct wl_listener client_destroy_listener = {.notify =
                                                    sl_client_destroy_notify};
  int sv[2];
  pid_t pid;
  int virtwl_display_fd = -1;
  int xdisplay = -1;
  int master = 0;
  int client_fd = -1;
  int rv;
  int i;

  for (i = 1; i < argc; ++i) {
    const char* arg = argv[i];
    if (strcmp(arg, "--help") == 0 || strcmp(arg, "-h") == 0 ||
        strcmp(arg, "-?") == 0) {
      sl_print_usage();
      return EXIT_SUCCESS;
    }
    if (strcmp(arg, "--version") == 0 || strcmp(arg, "-v") == 0) {
      printf("Version: %s\n", SOMMELIER_VERSION);
      return EXIT_SUCCESS;
    }
    if (strstr(arg, "--master") == arg) {
      master = 1;
    } else if (strstr(arg, "--socket") == arg) {
      socket_name = sl_arg_value(arg);
    } else if (strstr(arg, "--display") == arg) {
      display = sl_arg_value(arg);
    } else if (strstr(arg, "--shm-driver") == arg) {
      shm_driver = sl_arg_value(arg);
    } else if (strstr(arg, "--data-driver") == arg) {
      data_driver = sl_arg_value(arg);
    } else if (strstr(arg, "--peer-pid") == arg) {
      ctx.peer_pid = atoi(sl_arg_value(arg));
    } else if (strstr(arg, "--peer-cmd-prefix") == arg) {
      peer_cmd_prefix = sl_arg_value(arg);
    } else if (strstr(arg, "--xwayland-cmd-prefix") == arg) {
      xwayland_cmd_prefix = sl_arg_value(arg);
    } else if (strstr(arg, "--client-fd") == arg) {
      client_fd = atoi(sl_arg_value(arg));
    } else if (strstr(arg, "--scale") == arg) {
      scale = sl_arg_value(arg);
    } else if (strstr(arg, "--dpi") == arg) {
      dpi = sl_arg_value(arg);
    } else if (strstr(arg, "--accelerators") == arg) {
      accelerators = sl_arg_value(arg);
    } else if (strstr(arg, "--application-id") == arg) {
      ctx.application_id = sl_arg_value(arg);
    } else if (strstr(arg, "-X") == arg) {
      ctx.xwayland = 1;
    } else if (strstr(arg, "--x-display") == arg) {
      xdisplay = atoi(sl_arg_value(arg));
      // Automatically enable X forwarding if X display is specified.
      ctx.xwayland = 1;
    } else if (strstr(arg, "--xwayland-path") == arg) {
      xwayland_path = sl_arg_value(arg);
    } else if (strstr(arg, "--xwayland-gl-driver-path") == arg) {
      xwayland_gl_driver_path = sl_arg_value(arg);
    } else if (strstr(arg, "--no-exit-with-child") == arg) {
      ctx.exit_with_child = 0;
    } else if (strstr(arg, "--sd-notify") == arg) {
      ctx.sd_notify = sl_arg_value(arg);
    } else if (strstr(arg, "--no-clipboard-manager") == arg) {
      clipboard_manager = "0";
    } else if (strstr(arg, "--frame-color") == arg) {
      frame_color = sl_arg_value(arg);
    } else if (strstr(arg, "--dark-frame-color") == arg) {
      dark_frame_color = sl_arg_value(arg);
    } else if (strstr(arg, "--virtwl-device") == arg) {
      virtwl_device = sl_arg_value(arg);
    } else if (strstr(arg, "--drm-device") == arg) {
      drm_device = sl_arg_value(arg);
    } else if (strstr(arg, "--glamor") == arg) {
      glamor = "1";
    } else if (strstr(arg, "--x-auth") == arg) {
      xauth_path = sl_arg_value(arg);
    } else if (strstr(arg, "--x-font-path") == arg) {
      xfont_path = sl_arg_value(arg);
    } else if (arg[0] == '-') {
      if (strcmp(arg, "--") == 0) {
        ctx.runprog = &argv[i + 1];
        break;
      }
      // Don't exit on unknown options so we can have forward compatibility
      // with new flags introduced.
      fprintf(stderr, "Option `%s' is unknown, ignoring.\n", arg);
    } else {
      ctx.runprog = &argv[i];
      break;
    }
  }

  runtime_dir = getenv("XDG_RUNTIME_DIR");
  if (!runtime_dir) {
    fprintf(stderr, "error: XDG_RUNTIME_DIR not set in the environment\n");
    return EXIT_FAILURE;
  }

  if (master) {
    char* lock_addr;
    struct sockaddr_un addr;
    struct sigaction sa;
    struct stat sock_stat;
    int lock_fd;
    int sock_fd;

    addr.sun_family = AF_LOCAL;
    snprintf(addr.sun_path, sizeof(addr.sun_path), "%s/%s", runtime_dir,
             socket_name);

    lock_addr = sl_xasprintf("%s%s", addr.sun_path, LOCK_SUFFIX);

    lock_fd = open(lock_addr, O_CREAT | O_CLOEXEC,
                   (S_IRUSR | S_IWUSR | S_IRGRP | S_IWGRP));
    assert(lock_fd >= 0);

    rv = flock(lock_fd, LOCK_EX | LOCK_NB);
    if (rv < 0) {
      fprintf(stderr,
              "error: unable to lock %s, is another compositor running?\n",
              lock_addr);
      return EXIT_FAILURE;
    }
    free(lock_addr);

    rv = stat(addr.sun_path, &sock_stat);
    if (rv >= 0) {
      if (sock_stat.st_mode & (S_IWUSR | S_IWGRP))
        unlink(addr.sun_path);
    } else {
      assert(errno == ENOENT);
    }

    sock_fd = socket(PF_LOCAL, SOCK_STREAM, 0);
    assert(sock_fd >= 0);

    rv = bind(sock_fd, (struct sockaddr*)&addr,
              offsetof(struct sockaddr_un, sun_path) + strlen(addr.sun_path));
    assert(rv >= 0);

    rv = listen(sock_fd, 128);
    assert(rv >= 0);

    // Spawn optional child process before we notify systemd that we're ready
    // to accept connections. WAYLAND_DISPLAY will be set but any attempt to
    // connect to this socket at this time will fail.
    if (ctx.runprog && ctx.runprog[0]) {
      pid = fork();
      assert(pid != -1);
      if (pid == 0) {
        setenv("WAYLAND_DISPLAY", socket_name, 1);
        sl_execvp(ctx.runprog[0], ctx.runprog, -1);
        _exit(EXIT_FAILURE);
      }
      while (waitpid(-1, NULL, WNOHANG) != pid)
        continue;
    }

    if (ctx.sd_notify)
      sl_sd_notify(ctx.sd_notify);

    sa.sa_handler = sl_sigchld_handler;
    sigemptyset(&sa.sa_mask);
    sa.sa_flags = SA_RESTART;
    rv = sigaction(SIGCHLD, &sa, NULL);
    assert(rv >= 0);

    do {
      struct ucred ucred;
      socklen_t length = sizeof(addr);

      client_fd = accept(sock_fd, (struct sockaddr*)&addr, &length);
      if (client_fd < 0) {
        fprintf(stderr, "error: failed to accept: %m\n");
        continue;
      }

      ucred.pid = -1;
      length = sizeof(ucred);
      rv = getsockopt(client_fd, SOL_SOCKET, SO_PEERCRED, &ucred, &length);

      pid = fork();
      assert(pid != -1);
      if (pid == 0) {
        char* client_fd_str;
        char* peer_pid_str;
        //char* peer_cmd_prefix_str;
        char* args[64];
        int i = 0, j;

        close(sock_fd);
        close(lock_fd);

        if (!peer_cmd_prefix)
          peer_cmd_prefix = PEER_CMD_PREFIX;

        /*
        if (peer_cmd_prefix) {
          peer_cmd_prefix_str = sl_xasprintf("%s", peer_cmd_prefix);

          i = sl_parse_cmd_prefix(peer_cmd_prefix_str, 32, args);
          if (i > 32) {
            fprintf(stderr, "error: too many arguments in cmd prefix: %d\n", i);
            i = 0;
          }
        }
        */

        args[i++] = argv[0];
        peer_pid_str = sl_xasprintf("--peer-pid=%d", ucred.pid);
        args[i++] = peer_pid_str;
        client_fd_str = sl_xasprintf("--client-fd=%d", client_fd);
        args[i++] = client_fd_str;

        // forward some flags.
        for (j = 1; j < argc; ++j) {
          char* arg = argv[j];
          if (strstr(arg, "--display") == arg ||
              strstr(arg, "--scale") == arg ||
              strstr(arg, "--accelerators") == arg ||
              strstr(arg, "--virtwl-device") == arg ||
              strstr(arg, "--drm-device") == arg ||
              strstr(arg, "--shm-driver") == arg ||
              strstr(arg, "--data-driver") == arg) {
            args[i++] = arg;
          }
        }

        args[i++] = NULL;

        execvp(args[0], args);
        _exit(EXIT_FAILURE);
      }
      close(client_fd);
    } while (1);

    // Control should never reach here.
    assert(false);
  }

  if (client_fd == -1) {
    if (!ctx.runprog || !ctx.runprog[0]) {
      sl_print_usage();
      return EXIT_FAILURE;
    }
  }

  if (ctx.xwayland) {
    assert(client_fd == -1);

    ctx.clipboard_manager = 1;
    if (clipboard_manager)
      ctx.clipboard_manager = !!strcmp(clipboard_manager, "0");
  }

  if (scale) {
    ctx.desired_scale = atof(scale);
    // Round to integer scale until we detect wp_viewporter support.
    ctx.scale = MIN(MAX_SCALE, MAX(MIN_SCALE, round(ctx.desired_scale)));
  }

  if (!frame_color)
    frame_color = FRAME_COLOR;

  if (frame_color) {
    int r, g, b;
    if (sscanf(frame_color, "#%02x%02x%02x", &r, &g, &b) == 3)
      ctx.frame_color = 0xff000000 | (r << 16) | (g << 8) | (b << 0);
  }

  if (!dark_frame_color)
    dark_frame_color = DARK_FRAME_COLOR;

  if (dark_frame_color) {
    int r, g, b;
    if (sscanf(dark_frame_color, "#%02x%02x%02x", &r, &g, &b) == 3)
      ctx.dark_frame_color = 0xff000000 | (r << 16) | (g << 8) | (b << 0);
  }

  // Handle broken pipes without signals that kill the entire process.
  signal(SIGPIPE, SIG_IGN);

  ctx.host_display = wl_display_create();
  assert(ctx.host_display);

  event_loop = wl_display_get_event_loop(ctx.host_display);

  if (!virtwl_device)
    virtwl_device = VIRTWL_DEVICE;

  if (virtwl_device) {
    struct virtwl_ioctl_new new_ctx = {
        .type = VIRTWL_IOCTL_NEW_CTX,
        .fd = -1,
        .flags = 0,
        .size = 0,
    };

    ctx.virtwl_fd = open(virtwl_device, O_RDWR);
    if (ctx.virtwl_fd == -1) {
      fprintf(stderr, "error: could not open %s (%s)\n", virtwl_device,
              strerror(errno));
      return EXIT_FAILURE;
    }

    // We use a virtwl context unless display was explicitly specified.
    // WARNING: It's critical that we never call wl_display_roundtrip
    // as we're not spawning a new thread to handle forwarding. Calling
    // wl_display_roundtrip will cause a deadlock.
    if (!display) {
      int vws[2];

      // Connection to virtwl channel.
      rv = socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, vws);
      assert(!rv);

      ctx.virtwl_socket_fd = vws[0];
      virtwl_display_fd = vws[1];

      rv = ioctl(ctx.virtwl_fd, VIRTWL_IOCTL_NEW, &new_ctx);
      if (rv) {
        fprintf(stderr, "error: failed to create virtwl context: %s\n",
                strerror(errno));
        return EXIT_FAILURE;
      }

      ctx.virtwl_ctx_fd = new_ctx.fd;

      ctx.virtwl_socket_event_source = wl_event_loop_add_fd(
          event_loop, ctx.virtwl_socket_fd, WL_EVENT_READABLE,
          sl_handle_virtwl_socket_event, &ctx);
      ctx.virtwl_ctx_event_source =
          wl_event_loop_add_fd(event_loop, ctx.virtwl_ctx_fd, WL_EVENT_READABLE,
                               sl_handle_virtwl_ctx_event, &ctx);
    }
  }

  if (drm_device) {
    int drm_fd = open(drm_device, O_RDWR | O_CLOEXEC);
    if (drm_fd == -1) {
      fprintf(stderr, "error: could not open %s (%s)\n", drm_device,
              strerror(errno));
      return EXIT_FAILURE;
    }

    ctx.gbm = gbm_create_device(drm_fd);
    if (!ctx.gbm) {
      fprintf(stderr, "error: couldn't get display device\n");
      return EXIT_FAILURE;
    }

    ctx.drm_device = drm_device;
  }

  if (!shm_driver)
    shm_driver = ctx.xwayland ? XWAYLAND_SHM_DRIVER : SHM_DRIVER;

  if (shm_driver) {
    if (strcmp(shm_driver, "dmabuf") == 0) {
      if (!ctx.drm_device) {
        fprintf(stderr, "error: need drm device for dmabuf driver\n");
        return EXIT_FAILURE;
      }
      ctx.shm_driver = SHM_DRIVER_DMABUF;
    } else if (strcmp(shm_driver, "virtwl") == 0 ||
               strcmp(shm_driver, "virtwl-dmabuf") == 0) {
      if (ctx.virtwl_fd == -1) {
        fprintf(stderr, "error: need device for virtwl driver\n");
        return EXIT_FAILURE;
      }
      ctx.shm_driver = strcmp(shm_driver, "virtwl") ? SHM_DRIVER_VIRTWL_DMABUF
                                                    : SHM_DRIVER_VIRTWL;
      // Check for compatibility with virtwl-dmabuf.
      if (ctx.shm_driver == SHM_DRIVER_VIRTWL_DMABUF) {
        struct virtwl_ioctl_new new_dmabuf = {
            .type = VIRTWL_IOCTL_NEW_DMABUF,
            .fd = -1,
            .flags = 0,
            .dmabuf =
                {
                    .width = 0,
                    .height = 0,
                    .format = 0,
                },
        };
        if (ioctl(ctx.virtwl_fd, VIRTWL_IOCTL_NEW, &new_dmabuf) == -1 &&
            errno == ENOTTY) {
          fprintf(stderr,
                  "warning: virtwl-dmabuf driver not supported by host, using "
                  "virtwl instead\n");
          ctx.shm_driver = SHM_DRIVER_VIRTWL;
        } else if (new_dmabuf.fd >= 0) {
          // Close the returned dmabuf fd in case the invalid dmabuf metadata
          // given above actually manages to return an fd successfully.
          close(new_dmabuf.fd);
        }
      }
    }
  } else if (ctx.drm_device) {
    ctx.shm_driver = SHM_DRIVER_DMABUF;
  } else if (ctx.virtwl_fd != -1) {
    ctx.shm_driver = SHM_DRIVER_VIRTWL_DMABUF;
  }

  if (data_driver) {
    if (strcmp(data_driver, "virtwl") == 0) {
      if (ctx.virtwl_fd == -1) {
        fprintf(stderr, "error: need device for virtwl driver\n");
        return EXIT_FAILURE;
      }
      ctx.data_driver = DATA_DRIVER_VIRTWL;
    }
  } else if (ctx.virtwl_fd != -1) {
    ctx.data_driver = DATA_DRIVER_VIRTWL;
  }

  // Use well known values for DPI by default with Xwayland.
  if (!dpi && ctx.xwayland)
    dpi = "72,96,160,240,320,480";

  wl_array_init(&ctx.dpi);
  if (dpi) {
    char* str = strdup(dpi);
    char* token = strtok(str, ",");
    int* p;

    while (token) {
      p = wl_array_add(&ctx.dpi, sizeof *p);
      assert(p);
      *p = MAX(MIN_DPI, MIN(atoi(token), MAX_DPI));
      token = strtok(NULL, ",");
    }
    free(str);
  }

  if (ctx.runprog || ctx.xwayland) {
    // Wayland connection from client.
    rv = socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, sv);
    assert(!rv);

    client_fd = sv[0];
  }

  // The success of this depends on xkb-data being installed.
  ctx.xkb_context = xkb_context_new(0);
  if (!ctx.xkb_context) {
    fprintf(stderr, "error: xkb_context_new failed. xkb-data missing?\n");
    return EXIT_FAILURE;
  }

  if (virtwl_display_fd != -1) {
    ctx.display = wl_display_connect_to_fd(virtwl_display_fd);
  } else {
    if (display == NULL)
      display = getenv("WAYLAND_DISPLAY");
    if (display == NULL)
      display = "wayland-0";

    ctx.display = wl_display_connect(display);
  }

  if (!ctx.display) {
    fprintf(stderr, "error: failed to connect to %s\n", display);
    return EXIT_FAILURE;
  }

  wl_list_init(&ctx.accelerators);
  wl_list_init(&ctx.registries);
  wl_list_init(&ctx.globals);
  wl_list_init(&ctx.outputs);
  wl_list_init(&ctx.seats);
  wl_list_init(&ctx.windows);
  wl_list_init(&ctx.unpaired_windows);
  wl_list_init(&ctx.host_outputs);
  wl_list_init(&ctx.selection_data_source_send_pending);

  // Parse the list of accelerators that should be reserved by the
  // compositor. Format is "|MODIFIERS|KEYSYM", where MODIFIERS is a
  // list of modifier names (E.g. <Control><Alt>) and KEYSYM is an
  // XKB key symbol name (E.g Delete).
  if (accelerators) {
    uint32_t modifiers = 0;

    while (*accelerators) {
      if (*accelerators == ',') {
        accelerators++;
      } else if (*accelerators == '<') {
        if (strncmp(accelerators, "<Control>", 9) == 0) {
          modifiers |= CONTROL_MASK;
          accelerators += 9;
        } else if (strncmp(accelerators, "<Alt>", 5) == 0) {
          modifiers |= ALT_MASK;
          accelerators += 5;
        } else if (strncmp(accelerators, "<Shift>", 7) == 0) {
          modifiers |= SHIFT_MASK;
          accelerators += 7;
        } else {
          fprintf(stderr, "error: invalid modifier\n");
          return EXIT_FAILURE;
        }
      } else {
        struct sl_accelerator* accelerator;
        const char* end = strchrnul(accelerators, ',');
        char* name = strndup(accelerators, end - accelerators);

        accelerator = malloc(sizeof(*accelerator));
        accelerator->modifiers = modifiers;
        accelerator->symbol =
            xkb_keysym_from_name(name, XKB_KEYSYM_CASE_INSENSITIVE);
        if (accelerator->symbol == XKB_KEY_NoSymbol) {
          fprintf(stderr, "error: invalid key symbol\n");
          return EXIT_FAILURE;
        }

        wl_list_insert(&ctx.accelerators, &accelerator->link);

        modifiers = 0;
        accelerators = end;
        free(name);
      }
    }
  }

  ctx.display_event_source =
      wl_event_loop_add_fd(event_loop, wl_display_get_fd(ctx.display),
                           WL_EVENT_READABLE, sl_handle_event, &ctx);

  wl_registry_add_listener(wl_display_get_registry(ctx.display),
                           &sl_registry_listener, &ctx);

  ctx.client = wl_client_create(ctx.host_display, client_fd);

  // Replace the core display implementation. This is needed in order to
  // implement sync handler properly.
  sl_set_display_implementation(&ctx);

  if (ctx.runprog || ctx.xwayland) {
    ctx.sigchld_event_source =
        wl_event_loop_add_signal(event_loop, SIGCHLD, sl_handle_sigchld, &ctx);

    // Unset DISPLAY to prevent X clients from connecting to an existing X
    // server when X forwarding is not enabled.
    unsetenv("DISPLAY");
    // Set WAYLAND_DISPLAY to value that is guaranteed to not point to a
    // valid wayland compositor socket name. Resetting WAYLAND_DISPLAY is
    // insufficient as clients will attempt to connect to wayland-0 if
    // it's not set.
    setenv("WAYLAND_DISPLAY", ".", 1);

    if (ctx.xwayland) {
      int ds[2], wm[2];

      // Xwayland display ready socket.
      rv = socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, ds);
      assert(!rv);

      ctx.display_ready_event_source =
          wl_event_loop_add_fd(event_loop, ds[0], WL_EVENT_READABLE,
                               sl_handle_display_ready_event, &ctx);

      // X connection to Xwayland.
      rv = socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, wm);
      assert(!rv);

      ctx.wm_fd = wm[0];

      pid = fork();
      assert(pid != -1);
      if (pid == 0) {
        char* display_fd_str;
        char* wm_fd_str;
        char* xwayland_cmd_prefix_str;
        char* args[64];
        int i = 0;
        int fd;

        if (xwayland_cmd_prefix) {
          xwayland_cmd_prefix_str = sl_xasprintf("%s", xwayland_cmd_prefix);

          i = sl_parse_cmd_prefix(xwayland_cmd_prefix_str, 32, args);
          if (i > 32) {
            fprintf(stderr, "error: too many arguments in cmd prefix: %d\n", i);
            i = 0;
          }
        }

        args[i++] = sl_xasprintf("%s", xwayland_path ?: XWAYLAND_PATH);

        fd = dup(ds[1]);
        display_fd_str = sl_xasprintf("%d", fd);
        fd = dup(wm[1]);
        wm_fd_str = sl_xasprintf("%d", fd);

        if (xdisplay > 0) {
          args[i++] = sl_xasprintf(":%d", xdisplay);
        }
        args[i++] = "-nolisten";
        args[i++] = "tcp";
        args[i++] = "-rootless";
        // Use software rendering unless we have a DRM device and glamor is
        // enabled.
        if (!ctx.drm_device || !glamor || !strcmp(glamor, "0"))
          args[i++] = "-shm";
        args[i++] = "-displayfd";
        args[i++] = display_fd_str;
        args[i++] = "-wm";
        args[i++] = wm_fd_str;
        if (xauth_path) {
          args[i++] = "-auth";
          args[i++] = sl_xasprintf("%s", xauth_path);
        }
        if (xfont_path) {
          args[i++] = "-fp";
          args[i++] = sl_xasprintf("%s", xfont_path);
        }
        args[i++] = NULL;

        // If a path is explicitly specified via command line or environment
        // use that instead of the compiled in default.  In either case, only
        // set the environment variable if the value specified is non-empty.
        if (xwayland_gl_driver_path) {
          if (*xwayland_gl_driver_path) {
            setenv("LIBGL_DRIVERS_PATH", xwayland_gl_driver_path, 1);
          }
        } else if (XWAYLAND_GL_DRIVER_PATH && *XWAYLAND_GL_DRIVER_PATH) {
          setenv("LIBGL_DRIVERS_PATH", XWAYLAND_GL_DRIVER_PATH, 1);
        }

        sl_execvp(args[0], args, sv[1]);
        _exit(EXIT_FAILURE);
      }
      close(wm[1]);
      ctx.xwayland_pid = pid;
    } else {
      pid = fork();
      assert(pid != -1);
      if (pid == 0) {
        sl_execvp(ctx.runprog[0], ctx.runprog, sv[1]);
        _exit(EXIT_FAILURE);
      }
      ctx.child_pid = pid;
    }
    close(sv[1]);
  }

  wl_client_add_destroy_listener(ctx.client, &client_destroy_listener);

  do {
    wl_display_flush_clients(ctx.host_display);
    if (ctx.connection) {
      if (ctx.needs_set_input_focus) {
        sl_set_input_focus(&ctx, ctx.host_focus_window);
        ctx.needs_set_input_focus = 0;
      }
      xcb_flush(ctx.connection);
    }
    if (wl_display_flush(ctx.display) < 0)
      return EXIT_FAILURE;
  } while (wl_event_loop_dispatch(event_loop, -1) != -1);

  return EXIT_SUCCESS;
}
