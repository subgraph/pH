// Copyright 2018 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#ifndef VM_TOOLS_SOMMELIER_SOMMELIER_H_
#define VM_TOOLS_SOMMELIER_SOMMELIER_H_

#include <sys/types.h>
#include <wayland-server.h>
#include <wayland-util.h>
#include <xcb/xcb.h>
#include <xkbcommon/xkbcommon.h>

#include "config.h"

#define SOMMELIER_VERSION "0.20"

#define MIN(a, b) (((a) < (b)) ? (a) : (b))
#define MAX(a, b) (((a) > (b)) ? (a) : (b))

#define ARRAY_SIZE(a) (sizeof(a) / sizeof(a[0]))

#define UNUSED(x) ((void)(x))

#define CONTROL_MASK (1 << 0)
#define ALT_MASK (1 << 1)
#define SHIFT_MASK (1 << 2)

struct sl_global;
struct sl_compositor;
struct sl_shm;
struct sl_shell;
struct sl_data_device_manager;
struct sl_data_offer;
struct sl_data_source;
struct sl_xdg_shell;
struct sl_subcompositor;
struct sl_aura_shell;
struct sl_viewporter;
struct sl_linux_dmabuf;
struct sl_keyboard_extension;
struct sl_text_input_manager;
struct sl_relative_pointer_manager;
struct sl_window;
struct zaura_shell;
struct zcr_keyboard_extension_v1;

enum {
  ATOM_WM_S0,
  ATOM_WM_PROTOCOLS,
  ATOM_WM_STATE,
  ATOM_WM_CHANGE_STATE,
  ATOM_WM_DELETE_WINDOW,
  ATOM_WM_TAKE_FOCUS,
  ATOM_WM_CLIENT_LEADER,
  ATOM_WL_SURFACE_ID,
  ATOM_UTF8_STRING,
  ATOM_MOTIF_WM_HINTS,
  ATOM_NET_FRAME_EXTENTS,
  ATOM_NET_STARTUP_ID,
  ATOM_NET_SUPPORTING_WM_CHECK,
  ATOM_NET_WM_NAME,
  ATOM_NET_WM_MOVERESIZE,
  ATOM_NET_WM_STATE,
  ATOM_NET_WM_STATE_FULLSCREEN,
  ATOM_NET_WM_STATE_MAXIMIZED_VERT,
  ATOM_NET_WM_STATE_MAXIMIZED_HORZ,
  ATOM_CLIPBOARD,
  ATOM_CLIPBOARD_MANAGER,
  ATOM_TARGETS,
  ATOM_TIMESTAMP,
  ATOM_TEXT,
  ATOM_INCR,
  ATOM_WL_SELECTION,
  ATOM_GTK_THEME_VARIANT,
  ATOM_LAST = ATOM_GTK_THEME_VARIANT,
};

enum {
  SHM_DRIVER_NOOP,
  SHM_DRIVER_DMABUF,
  SHM_DRIVER_VIRTWL,
  SHM_DRIVER_VIRTWL_DMABUF,
};

enum {
  DATA_DRIVER_NOOP,
  DATA_DRIVER_VIRTWL,
};

struct sl_context {
  char** runprog;
  struct wl_display* display;
  struct wl_display* host_display;
  struct wl_client* client;
  struct sl_compositor* compositor;
  struct sl_subcompositor* subcompositor;
  struct sl_shm* shm;
  struct sl_shell* shell;
  struct sl_data_device_manager* data_device_manager;
  struct sl_xdg_shell* xdg_shell;
  struct sl_aura_shell* aura_shell;
  struct sl_viewporter* viewporter;
  struct sl_linux_dmabuf* linux_dmabuf;
  struct sl_keyboard_extension* keyboard_extension;
  struct sl_text_input_manager* text_input_manager;
  struct sl_relative_pointer_manager* relative_pointer_manager;
  struct wl_list outputs;
  struct wl_list seats;
  struct wl_event_source* display_event_source;
  struct wl_event_source* display_ready_event_source;
  struct wl_event_source* sigchld_event_source;
  struct wl_array dpi;
  int shm_driver;
  int data_driver;
  int wm_fd;
  int virtwl_fd;
  int virtwl_ctx_fd;
  int virtwl_socket_fd;
  struct wl_event_source* virtwl_ctx_event_source;
  struct wl_event_source* virtwl_socket_event_source;
  const char* drm_device;
  struct gbm_device* gbm;
  int xwayland;
  pid_t xwayland_pid;
  pid_t child_pid;
  pid_t peer_pid;
  struct xkb_context* xkb_context;
  struct wl_list accelerators;
  struct wl_list registries;
  struct wl_list globals;
  struct wl_list host_outputs;
  int next_global_id;
  xcb_connection_t* connection;
  struct wl_event_source* connection_event_source;
  const xcb_query_extension_reply_t* xfixes_extension;
  xcb_screen_t* screen;
  xcb_window_t window;
  struct wl_list windows, unpaired_windows;
  struct sl_window* host_focus_window;
  int needs_set_input_focus;
  double desired_scale;
  double scale;
  const char* application_id;
  int exit_with_child;
  const char* sd_notify;
  int clipboard_manager;
  uint32_t frame_color;
  uint32_t dark_frame_color;
  struct sl_host_seat* default_seat;
  xcb_window_t selection_window;
  xcb_window_t selection_owner;
  int selection_incremental_transfer;
  xcb_selection_request_event_t selection_request;
  xcb_timestamp_t selection_timestamp;
  struct wl_data_device* selection_data_device;
  struct sl_data_offer* selection_data_offer;
  struct sl_data_source* selection_data_source;
  int selection_data_source_send_fd;
  struct wl_list selection_data_source_send_pending;
  struct wl_event_source* selection_send_event_source;
  xcb_get_property_reply_t* selection_property_reply;
  int selection_property_offset;
  struct wl_event_source* selection_event_source;
  xcb_atom_t selection_data_type;
  struct wl_array selection_data;
  int selection_data_offer_receive_fd;
  int selection_data_ack_pending;
  union {
    const char* name;
    xcb_intern_atom_cookie_t cookie;
    xcb_atom_t value;
  } atoms[ATOM_LAST + 1];
  xcb_visualid_t visual_ids[256];
  xcb_colormap_t colormaps[256];
};

struct sl_compositor {
  struct sl_context* ctx;
  uint32_t id;
  uint32_t version;
  struct sl_global* host_global;
  struct wl_compositor* internal;
};

struct sl_shm {
  struct sl_context* ctx;
  uint32_t id;
  struct sl_global* host_global;
  struct wl_shm* internal;
};

struct sl_seat {
  struct sl_context* ctx;
  uint32_t id;
  uint32_t version;
  struct sl_global* host_global;
  uint32_t last_serial;
  struct wl_list link;
};

struct sl_host_pointer {
  struct sl_seat* seat;
  struct wl_resource* resource;
  struct wl_pointer* proxy;
  struct wl_resource* focus_resource;
  struct wl_listener focus_resource_listener;
  uint32_t focus_serial;
};

struct sl_relative_pointer_manager {
  struct sl_context* ctx;
  uint32_t id;
  struct sl_global* host_global;
  struct zwp_relative_pointer_manager_v1* internal;
};

struct sl_viewport {
  struct wl_list link;
  wl_fixed_t src_x;
  wl_fixed_t src_y;
  wl_fixed_t src_width;
  wl_fixed_t src_height;
  int32_t dst_width;
  int32_t dst_height;
};

struct sl_host_callback {
  struct wl_resource* resource;
  struct wl_callback* proxy;
};

struct sl_host_surface {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct wl_surface* proxy;
  struct wp_viewport* viewport;
  uint32_t contents_width;
  uint32_t contents_height;
  int32_t contents_scale;
  struct wl_list contents_viewport;
  struct sl_mmap* contents_shm_mmap;
  int has_role;
  int has_output;
  uint32_t last_event_serial;
  struct sl_output_buffer* current_buffer;
  struct wl_list released_buffers;
  struct wl_list busy_buffers;
};

struct sl_host_buffer {
  struct wl_resource* resource;
  struct wl_buffer* proxy;
  uint32_t width;
  uint32_t height;
  struct sl_mmap* shm_mmap;
  uint32_t shm_format;
  struct sl_sync_point* sync_point;
};

struct sl_data_source_send_request {
  int fd;
  xcb_intern_atom_cookie_t cookie;
  struct sl_data_source* data_source;
  struct wl_list link;
};

struct sl_subcompositor {
  struct sl_context* ctx;
  uint32_t id;
  struct sl_global* host_global;
};

struct sl_shell {
  struct sl_context* ctx;
  uint32_t id;
  struct sl_global* host_global;
};

struct sl_output {
  struct sl_context* ctx;
  uint32_t id;
  uint32_t version;
  struct sl_global* host_global;
  struct wl_list link;
};

struct sl_host_output {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct wl_output* proxy;
  struct zaura_output* aura_output;
  int internal;
  int x;
  int y;
  int physical_width;
  int physical_height;
  int subpixel;
  char* make;
  char* model;
  int transform;
  uint32_t flags;
  int width;
  int height;
  int refresh;
  int scale_factor;
  int current_scale;
  int preferred_scale;
  int device_scale_factor;
  int expecting_scale;
  struct wl_list link;
};

struct sl_host_seat {
  struct sl_seat* seat;
  struct wl_resource* resource;
  struct wl_seat* proxy;
};

struct sl_accelerator {
  struct wl_list link;
  uint32_t modifiers;
  xkb_keysym_t symbol;
};

struct sl_keyboard_extension {
  struct sl_context* ctx;
  uint32_t id;
  struct zcr_keyboard_extension_v1* internal;
};

struct sl_data_device_manager {
  struct sl_context* ctx;
  uint32_t id;
  uint32_t version;
  struct sl_global* host_global;
  struct wl_data_device_manager* internal;
};

struct sl_data_offer {
  struct sl_context* ctx;
  struct wl_data_offer* internal;
  struct wl_array atoms;    // Contains xcb_atom_t
  struct wl_array cookies;  // Contains xcb_intern_atom_cookie_t
};

struct sl_text_input_manager {
  struct sl_context* ctx;
  uint32_t id;
  struct sl_global* host_global;
  struct zwp_text_input_manager_v1* internal;
};

struct sl_viewporter {
  struct sl_context* ctx;
  uint32_t id;
  struct sl_global* host_viewporter_global;
  struct wp_viewporter* internal;
};

struct sl_xdg_shell {
  struct sl_context* ctx;
  uint32_t id;
  struct sl_global* host_global;
  struct zxdg_shell_v6* internal;
};

struct sl_aura_shell {
  struct sl_context* ctx;
  uint32_t id;
  uint32_t version;
  struct sl_global* host_gtk_shell_global;
  struct zaura_shell* internal;
};

struct sl_linux_dmabuf {
  struct sl_context* ctx;
  uint32_t id;
  uint32_t version;
  struct sl_global* host_drm_global;
  struct zwp_linux_dmabuf_v1* internal;
};

struct sl_global {
  struct sl_context* ctx;
  const struct wl_interface* interface;
  uint32_t name;
  uint32_t version;
  void* data;
  wl_global_bind_func_t bind;
  struct wl_list link;
};

struct sl_host_registry {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct wl_list link;
};

typedef void (*sl_begin_end_access_func_t)(int fd);

struct sl_mmap {
  int refcount;
  int fd;
  void* addr;
  size_t size;
  size_t bpp;
  size_t num_planes;
  size_t offset[2];
  size_t stride[2];
  size_t y_ss[2];
  sl_begin_end_access_func_t begin_write;
  sl_begin_end_access_func_t end_write;
  struct wl_resource* buffer_resource;
};

typedef void (*sl_sync_func_t)(struct sl_context *ctx,
                               struct sl_sync_point* sync_point);

struct sl_sync_point {
  int fd;
  sl_sync_func_t sync;
};

struct sl_config {
  uint32_t serial;
  uint32_t mask;
  uint32_t values[5];
  uint32_t states_length;
  uint32_t states[3];
};

struct sl_window {
  struct sl_context* ctx;
  xcb_window_t id;
  xcb_window_t frame_id;
  uint32_t host_surface_id;
  int unpaired;
  int x;
  int y;
  int width;
  int height;
  int border_width;
  int depth;
  int managed;
  int realized;
  int activated;
  int allow_resize;
  xcb_window_t transient_for;
  xcb_window_t client_leader;
  int decorated;
  char* name;
  char* clazz;
  char* startup_id;
  int dark_frame;
  uint32_t size_flags;
  int min_width;
  int min_height;
  int max_width;
  int max_height;
  struct sl_config next_config;
  struct sl_config pending_config;
  struct zxdg_surface_v6* xdg_surface;
  struct zxdg_toplevel_v6* xdg_toplevel;
  struct zxdg_popup_v6* xdg_popup;
  struct zaura_surface* aura_surface;
  struct wl_list link;
};

struct sl_host_buffer* sl_create_host_buffer(struct wl_client* client,
                                             uint32_t id,
                                             struct wl_buffer* proxy,
                                             int32_t width,
                                             int32_t height);

struct sl_global* sl_global_create(struct sl_context* ctx,
                                   const struct wl_interface* interface,
                                   int version,
                                   void* data,
                                   wl_global_bind_func_t bind);

struct sl_global* sl_compositor_global_create(struct sl_context* ctx);

size_t sl_shm_bpp_for_shm_format(uint32_t format);

size_t sl_shm_num_planes_for_shm_format(uint32_t format);

struct sl_global* sl_shm_global_create(struct sl_context* ctx);

struct sl_global* sl_subcompositor_global_create(struct sl_context* ctx);

struct sl_global* sl_shell_global_create(struct sl_context* ctx);

double sl_output_aura_scale_factor_to_double(int scale_factor);

void sl_output_send_host_output_state(struct sl_host_output* host);

struct sl_global* sl_output_global_create(struct sl_output* output);

struct sl_global* sl_seat_global_create(struct sl_seat* seat);

struct sl_global* sl_relative_pointer_manager_global_create(
    struct sl_context* ctx);

struct sl_global* sl_data_device_manager_global_create(struct sl_context* ctx);

struct sl_global* sl_viewporter_global_create(struct sl_context* ctx);

struct sl_global* sl_xdg_shell_global_create(struct sl_context* ctx);

struct sl_global* sl_gtk_shell_global_create(struct sl_context* ctx);

struct sl_global* sl_drm_global_create(struct sl_context* ctx);

struct sl_global* sl_text_input_manager_global_create(struct sl_context* ctx);

void sl_set_display_implementation(struct sl_context* ctx);

struct sl_mmap* sl_mmap_create(int fd,
                               size_t size,
                               size_t bpp,
                               size_t num_planes,
                               size_t offset0,
                               size_t stride0,
                               size_t offset1,
                               size_t stride1,
                               size_t y_ss0,
                               size_t y_ss1);
struct sl_mmap* sl_mmap_ref(struct sl_mmap* map);
void sl_mmap_unref(struct sl_mmap* map);

struct sl_sync_point* sl_sync_point_create(int fd);
void sl_sync_point_destroy(struct sl_sync_point* sync_point);

void sl_host_seat_added(struct sl_host_seat* host);
void sl_host_seat_removed(struct sl_host_seat* host);

void sl_restack_windows(struct sl_context* ctx, uint32_t focus_resource_id);

void sl_roundtrip(struct sl_context* ctx);

int sl_process_pending_configure_acks(struct sl_window* window,
                                      struct sl_host_surface* host_surface);

void sl_window_update(struct sl_window* window);

#endif  // VM_TOOLS_SOMMELIER_SOMMELIER_H_
