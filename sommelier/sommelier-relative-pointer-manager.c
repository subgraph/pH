// Copyright 2019 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "sommelier.h"

#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>
#include <wayland-server-core.h>

#include "relative-pointer-unstable-v1-server-protocol.h"
#include "relative-pointer-unstable-v1-client-protocol.h"

struct sl_host_relative_pointer_manager {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct zwp_relative_pointer_manager_v1* proxy;
};

struct sl_host_relative_pointer {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct zwp_relative_pointer_v1* proxy;
};

static void sl_relative_pointer_relative_motion(
    void* data,
    struct zwp_relative_pointer_v1* relative_pointer,
    uint32_t utime_hi,
    uint32_t utime_lo,
    wl_fixed_t dx,
    wl_fixed_t dy,
    wl_fixed_t dx_unaccel,
    wl_fixed_t dy_unaccel) {
  struct sl_host_relative_pointer* host =
      zwp_relative_pointer_v1_get_user_data(relative_pointer);

  zwp_relative_pointer_v1_send_relative_motion(
      host->resource, utime_hi, utime_lo, dx, dy, dx_unaccel, dy_unaccel);
}

static void sl_destroy_host_relative_pointer(struct wl_resource* resource) {
  struct sl_host_relative_pointer* host = wl_resource_get_user_data(resource);

  zwp_relative_pointer_v1_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_relative_pointer_destroy(struct wl_client* client,
                                        struct wl_resource* resource) {
  wl_resource_destroy(resource);
}

static struct zwp_relative_pointer_v1_listener sl_relative_pointer_listener = {
    sl_relative_pointer_relative_motion,
};

static struct zwp_relative_pointer_v1_interface
    sl_relative_pointer_implementation = {
        sl_relative_pointer_destroy,
};

static void sl_destroy_host_relative_pointer_manager(
    struct wl_resource* resource) {
  struct sl_host_relative_pointer_manager* host =
      wl_resource_get_user_data(resource);

  zwp_relative_pointer_manager_v1_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_relative_pointer_manager_destroy(struct wl_client* client,
                                                struct wl_resource* resource) {
  wl_resource_destroy(resource);
}

static void sl_relative_pointer_manager_get_relative_pointer(
    struct wl_client* client,
    struct wl_resource* resource,
    uint32_t id,
    struct wl_resource* pointer) {
  struct sl_host_relative_pointer_manager* host =
      wl_resource_get_user_data(resource);
  struct sl_host_pointer* host_pointer = wl_resource_get_user_data(pointer);
  struct wl_resource* relative_pointer_resource =
      wl_resource_create(client, &zwp_relative_pointer_v1_interface, 1, id);
  struct sl_host_relative_pointer* relative_pointer_host;

  relative_pointer_host = malloc(sizeof(*relative_pointer_host));
  assert(relative_pointer_host);
  relative_pointer_host->resource = relative_pointer_resource;
  relative_pointer_host->ctx = host->ctx;
  relative_pointer_host->proxy =
      zwp_relative_pointer_manager_v1_get_relative_pointer(
          host->ctx->relative_pointer_manager->internal, host_pointer->proxy);
  wl_resource_set_implementation(
      relative_pointer_resource, &sl_relative_pointer_implementation,
      relative_pointer_host, sl_destroy_host_relative_pointer);
  zwp_relative_pointer_v1_set_user_data(relative_pointer_host->proxy,
                                        relative_pointer_host);
  zwp_relative_pointer_v1_add_listener(relative_pointer_host->proxy,
                                       &sl_relative_pointer_listener,
                                       relative_pointer_host);
}

static struct zwp_relative_pointer_manager_v1_interface
    sl_relative_pointer_manager_implementation = {
        sl_relative_pointer_manager_destroy,
        sl_relative_pointer_manager_get_relative_pointer,
};

static void sl_bind_host_relative_pointer_manager(struct wl_client* client,
                                                  void* data,
                                                  uint32_t version,
                                                  uint32_t id) {
  struct sl_context* ctx = (struct sl_context*)data;
  struct sl_relative_pointer_manager* relative_pointer_manager =
      ctx->relative_pointer_manager;
  struct sl_host_relative_pointer_manager* host;

  host = malloc(sizeof(*host));
  assert(host);
  host->ctx = ctx;
  host->resource = wl_resource_create(
      client, &zwp_relative_pointer_manager_v1_interface, 1, id);
  wl_resource_set_implementation(
      host->resource, &sl_relative_pointer_manager_implementation, host,
      sl_destroy_host_relative_pointer_manager);
  host->proxy = wl_registry_bind(wl_display_get_registry(ctx->display),
                                 relative_pointer_manager->id,
                                 &zwp_relative_pointer_manager_v1_interface,
                                 wl_resource_get_version(host->resource));
  zwp_relative_pointer_manager_v1_set_user_data(host->proxy, host);
}

struct sl_global* sl_relative_pointer_manager_global_create(
    struct sl_context* ctx) {
  return sl_global_create(ctx, &zwp_relative_pointer_manager_v1_interface, 1,
                          ctx, sl_bind_host_relative_pointer_manager);
}
