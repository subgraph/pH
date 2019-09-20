// Copyright 2018 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "sommelier.h"

#include <assert.h>
#include <errno.h>
#include <gbm.h>
#include <limits.h>
#include <linux/virtwl.h>
#include <pixman.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>
#include <wayland-client.h>
#include <wayland-util.h>

#include "drm-server-protocol.h"
#include "linux-dmabuf-unstable-v1-client-protocol.h"
#include "viewporter-client-protocol.h"

#define MIN_SIZE (INT_MIN / 10)
#define MAX_SIZE (INT_MAX / 10)

#define DMA_BUF_SYNC_READ (1 << 0)
#define DMA_BUF_SYNC_WRITE (2 << 0)
#define DMA_BUF_SYNC_RW (DMA_BUF_SYNC_READ | DMA_BUF_SYNC_WRITE)
#define DMA_BUF_SYNC_START (0 << 2)
#define DMA_BUF_SYNC_END (1 << 2)

#define DMA_BUF_BASE 'b'
#define DMA_BUF_IOCTL_SYNC _IOW(DMA_BUF_BASE, 0, struct dma_buf_sync)

struct sl_host_region {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct wl_region* proxy;
};

struct sl_host_compositor {
  struct sl_compositor* compositor;
  struct wl_resource* resource;
  struct wl_compositor* proxy;
};

struct sl_output_buffer {
  struct wl_list link;
  uint32_t width;
  uint32_t height;
  uint32_t format;
  struct wl_buffer* internal;
  struct sl_mmap* mmap;
  struct pixman_region32 damage;
  struct sl_host_surface* surface;
};

struct dma_buf_sync {
  __u64 flags;
};

static void sl_dmabuf_sync(int fd, __u64 flags) {
  struct dma_buf_sync sync = {0};
  int rv;

  sync.flags = flags;
  do {
    rv = ioctl(fd, DMA_BUF_IOCTL_SYNC, &sync);
  } while (rv == -1 && errno == EINTR);
}

static void sl_dmabuf_begin_write(int fd) {
  sl_dmabuf_sync(fd, DMA_BUF_SYNC_START | DMA_BUF_SYNC_WRITE);
}

static void sl_dmabuf_end_write(int fd) {
  sl_dmabuf_sync(fd, DMA_BUF_SYNC_END | DMA_BUF_SYNC_WRITE);
}

static void sl_virtwl_dmabuf_sync(int fd, __u32 flags) {
  struct virtwl_ioctl_dmabuf_sync sync = {0};
  int rv;

  sync.flags = flags;
  rv = ioctl(fd, VIRTWL_IOCTL_DMABUF_SYNC, &sync);
  assert(!rv);
  UNUSED(rv);
}

static void sl_virtwl_dmabuf_begin_write(int fd) {
  sl_virtwl_dmabuf_sync(fd, DMA_BUF_SYNC_START | DMA_BUF_SYNC_WRITE);
}

static void sl_virtwl_dmabuf_end_write(int fd) {
  sl_virtwl_dmabuf_sync(fd, DMA_BUF_SYNC_END | DMA_BUF_SYNC_WRITE);
}

static uint32_t sl_gbm_format_for_shm_format(uint32_t format) {
  switch (format) {
    case WL_SHM_FORMAT_NV12:
      return GBM_FORMAT_NV12;
    case WL_SHM_FORMAT_RGB565:
      return GBM_FORMAT_RGB565;
    case WL_SHM_FORMAT_ARGB8888:
      return GBM_FORMAT_ARGB8888;
    case WL_SHM_FORMAT_ABGR8888:
      return GBM_FORMAT_ABGR8888;
    case WL_SHM_FORMAT_XRGB8888:
      return GBM_FORMAT_XRGB8888;
    case WL_SHM_FORMAT_XBGR8888:
      return GBM_FORMAT_XBGR8888;
  }
  assert(0);
  return 0;
}

static uint32_t sl_drm_format_for_shm_format(int format) {
  switch (format) {
    case WL_SHM_FORMAT_NV12:
      return WL_DRM_FORMAT_NV12;
    case WL_SHM_FORMAT_RGB565:
      return WL_DRM_FORMAT_RGB565;
    case WL_SHM_FORMAT_ARGB8888:
      return WL_DRM_FORMAT_ARGB8888;
    case WL_SHM_FORMAT_ABGR8888:
      return WL_DRM_FORMAT_ABGR8888;
    case WL_SHM_FORMAT_XRGB8888:
      return WL_DRM_FORMAT_XRGB8888;
    case WL_SHM_FORMAT_XBGR8888:
      return WL_DRM_FORMAT_XBGR8888;
  }
  assert(0);
  return 0;
}

static void sl_output_buffer_destroy(struct sl_output_buffer* buffer) {
  wl_buffer_destroy(buffer->internal);
  sl_mmap_unref(buffer->mmap);
  pixman_region32_fini(&buffer->damage);
  wl_list_remove(&buffer->link);
  free(buffer);
}

static void sl_output_buffer_release(void* data, struct wl_buffer* buffer) {
  struct sl_output_buffer* output_buffer = wl_buffer_get_user_data(buffer);
  struct sl_host_surface* host_surface = output_buffer->surface;

  wl_list_remove(&output_buffer->link);
  wl_list_insert(&host_surface->released_buffers, &output_buffer->link);
}

static const struct wl_buffer_listener sl_output_buffer_listener = {
    sl_output_buffer_release};

static void sl_host_surface_destroy(struct wl_client* client,
                                    struct wl_resource* resource) {
  wl_resource_destroy(resource);
}

static void sl_host_surface_attach(struct wl_client* client,
                                   struct wl_resource* resource,
                                   struct wl_resource* buffer_resource,
                                   int32_t x,
                                   int32_t y) {
  struct sl_host_surface* host = wl_resource_get_user_data(resource);
  struct sl_host_buffer* host_buffer =
      buffer_resource ? wl_resource_get_user_data(buffer_resource) : NULL;
  struct wl_buffer* buffer_proxy = NULL;
  struct sl_window* window;
  double scale = host->ctx->scale;

  host->current_buffer = NULL;
  if (host->contents_shm_mmap) {
    sl_mmap_unref(host->contents_shm_mmap);
    host->contents_shm_mmap = NULL;
  }

  if (host_buffer) {
    host->contents_width = host_buffer->width;
    host->contents_height = host_buffer->height;
    buffer_proxy = host_buffer->proxy;
    if (host_buffer->shm_mmap)
      host->contents_shm_mmap = sl_mmap_ref(host_buffer->shm_mmap);
  }

  if (host->contents_shm_mmap) {
    while (!wl_list_empty(&host->released_buffers)) {
      host->current_buffer = wl_container_of(host->released_buffers.next,
                                             host->current_buffer, link);

      if (host->current_buffer->width == host_buffer->width &&
          host->current_buffer->height == host_buffer->height &&
          host->current_buffer->format == host_buffer->shm_format) {
        break;
      }

      sl_output_buffer_destroy(host->current_buffer);
      host->current_buffer = NULL;
    }

    // Allocate new output buffer.
    if (!host->current_buffer) {
      size_t width = host_buffer->width;
      size_t height = host_buffer->height;
      uint32_t shm_format = host_buffer->shm_format;
      size_t bpp = sl_shm_bpp_for_shm_format(shm_format);
      size_t num_planes = sl_shm_num_planes_for_shm_format(shm_format);

      host->current_buffer = malloc(sizeof(struct sl_output_buffer));
      assert(host->current_buffer);
      wl_list_insert(&host->released_buffers, &host->current_buffer->link);
      host->current_buffer->width = width;
      host->current_buffer->height = height;
      host->current_buffer->format = shm_format;
      host->current_buffer->surface = host;
      pixman_region32_init_rect(&host->current_buffer->damage, 0, 0, MAX_SIZE,
                                MAX_SIZE);

      switch (host->ctx->shm_driver) {
        case SHM_DRIVER_DMABUF: {
          struct zwp_linux_buffer_params_v1* buffer_params;
          struct gbm_bo* bo;
          int stride0;
          int fd;

          bo = gbm_bo_create(host->ctx->gbm, width, height,
                             sl_gbm_format_for_shm_format(shm_format),
                             GBM_BO_USE_SCANOUT | GBM_BO_USE_LINEAR);
          stride0 = gbm_bo_get_stride(bo);
          fd = gbm_bo_get_fd(bo);

          buffer_params = zwp_linux_dmabuf_v1_create_params(
              host->ctx->linux_dmabuf->internal);
          zwp_linux_buffer_params_v1_add(buffer_params, fd, 0, 0, stride0, 0,
                                         0);
          host->current_buffer->internal =
              zwp_linux_buffer_params_v1_create_immed(
                  buffer_params, width, height,
                  sl_drm_format_for_shm_format(shm_format), 0);
          zwp_linux_buffer_params_v1_destroy(buffer_params);

          host->current_buffer->mmap = sl_mmap_create(
              fd, height * stride0, bpp, 1, 0, stride0, 0, 0, 1, 0);
          host->current_buffer->mmap->begin_write = sl_dmabuf_begin_write;
          host->current_buffer->mmap->end_write = sl_dmabuf_end_write;

          gbm_bo_destroy(bo);
        } break;
        case SHM_DRIVER_VIRTWL: {
          size_t size = host_buffer->shm_mmap->size;
          struct virtwl_ioctl_new ioctl_new = {.type = VIRTWL_IOCTL_NEW_ALLOC,
                                               .fd = -1,
                                               .flags = 0,
                                               .size = size};
          struct wl_shm_pool* pool;
          int rv;

          rv = ioctl(host->ctx->virtwl_fd, VIRTWL_IOCTL_NEW, &ioctl_new);
          assert(rv == 0);
          UNUSED(rv);

          pool =
              wl_shm_create_pool(host->ctx->shm->internal, ioctl_new.fd, size);
          host->current_buffer->internal = wl_shm_pool_create_buffer(
              pool, 0, width, height, host_buffer->shm_mmap->stride[0],
              shm_format);
          wl_shm_pool_destroy(pool);

          host->current_buffer->mmap = sl_mmap_create(
              ioctl_new.fd, size, bpp, num_planes, 0,
              host_buffer->shm_mmap->stride[0],
              host_buffer->shm_mmap->offset[1] -
                  host_buffer->shm_mmap->offset[0],
              host_buffer->shm_mmap->stride[1], host_buffer->shm_mmap->y_ss[0],
              host_buffer->shm_mmap->y_ss[1]);
        } break;
        case SHM_DRIVER_VIRTWL_DMABUF: {
          uint32_t drm_format = sl_drm_format_for_shm_format(shm_format);
          struct virtwl_ioctl_new ioctl_new = {
              .type = VIRTWL_IOCTL_NEW_DMABUF,
              .fd = -1,
              .flags = 0,
              .dmabuf = {
                  .width = width, .height = height, .format = drm_format}};
          struct zwp_linux_buffer_params_v1* buffer_params;
          size_t size;
          int rv;

          rv = ioctl(host->ctx->virtwl_fd, VIRTWL_IOCTL_NEW, &ioctl_new);
          if (rv) {
            fprintf(stderr, "error: virtwl dmabuf allocation failed: %s\n",
                    strerror(errno));
            _exit(EXIT_FAILURE);
          }

          size = ioctl_new.dmabuf.stride0 * height;
          buffer_params = zwp_linux_dmabuf_v1_create_params(
              host->ctx->linux_dmabuf->internal);
          zwp_linux_buffer_params_v1_add(buffer_params, ioctl_new.fd, 0,
                                         ioctl_new.dmabuf.offset0,
                                         ioctl_new.dmabuf.stride0, 0, 0);
          if (num_planes > 1) {
            zwp_linux_buffer_params_v1_add(buffer_params, ioctl_new.fd, 1,
                                           ioctl_new.dmabuf.offset1,
                                           ioctl_new.dmabuf.stride1, 0, 0);
            size = MAX(size, ioctl_new.dmabuf.offset1 +
                                 ioctl_new.dmabuf.stride1 * height /
                                     host_buffer->shm_mmap->y_ss[1]);
          }
          host->current_buffer->internal =
              zwp_linux_buffer_params_v1_create_immed(buffer_params, width,
                                                      height, drm_format, 0);
          zwp_linux_buffer_params_v1_destroy(buffer_params);

          host->current_buffer->mmap = sl_mmap_create(
              ioctl_new.fd, size, bpp, num_planes, ioctl_new.dmabuf.offset0,
              ioctl_new.dmabuf.stride0, ioctl_new.dmabuf.offset1,
              ioctl_new.dmabuf.stride1, host_buffer->shm_mmap->y_ss[0],
              host_buffer->shm_mmap->y_ss[1]);
          host->current_buffer->mmap->begin_write =
              sl_virtwl_dmabuf_begin_write;
          host->current_buffer->mmap->end_write = sl_virtwl_dmabuf_end_write;
        } break;
      }

      assert(host->current_buffer->internal);
      assert(host->current_buffer->mmap);

      wl_buffer_set_user_data(host->current_buffer->internal,
                              host->current_buffer);
      wl_buffer_add_listener(host->current_buffer->internal,
                             &sl_output_buffer_listener, host->current_buffer);
    }
  }

  x /= scale;
  y /= scale;

  // TODO(davidriley): This should be done in the commit.
  if (host_buffer && host_buffer->sync_point) {
    host_buffer->sync_point->sync(host->ctx, host_buffer->sync_point);
  }

  if (host->current_buffer) {
    assert(host->current_buffer->internal);
    wl_surface_attach(host->proxy, host->current_buffer->internal, x, y);
  } else {
    wl_surface_attach(host->proxy, buffer_proxy, x, y);
  }

  wl_list_for_each(window, &host->ctx->windows, link) {
    if (window->host_surface_id == wl_resource_get_id(resource)) {
      while (sl_process_pending_configure_acks(window, host))
        continue;

      break;
    }
  }
}

static void sl_host_surface_damage(struct wl_client* client,
                                   struct wl_resource* resource,
                                   int32_t x,
                                   int32_t y,
                                   int32_t width,
                                   int32_t height) {
  struct sl_host_surface* host = wl_resource_get_user_data(resource);
  double scale = host->ctx->scale;
  struct sl_output_buffer* buffer;
  int64_t x1, y1, x2, y2;

  wl_list_for_each(buffer, &host->busy_buffers, link) {
    pixman_region32_union_rect(&buffer->damage, &buffer->damage, x, y, width,
                               height);
  }
  wl_list_for_each(buffer, &host->released_buffers, link) {
    pixman_region32_union_rect(&buffer->damage, &buffer->damage, x, y, width,
                               height);
  }

  x1 = x;
  y1 = y;
  x2 = x1 + width;
  y2 = y1 + height;

  // Enclosing rect after scaling and outset by one pixel to account for
  // potential filtering.
  x1 = MAX(MIN_SIZE, x1 - 1) / scale;
  y1 = MAX(MIN_SIZE, y1 - 1) / scale;
  x2 = ceil(MIN(x2 + 1, MAX_SIZE) / scale);
  y2 = ceil(MIN(y2 + 1, MAX_SIZE) / scale);

  wl_surface_damage(host->proxy, x1, y1, x2 - x1, y2 - y1);
}

static void sl_frame_callback_done(void* data,
                                   struct wl_callback* callback,
                                   uint32_t time) {
  struct sl_host_callback* host = wl_callback_get_user_data(callback);

  wl_callback_send_done(host->resource, time);
  wl_resource_destroy(host->resource);
}

static const struct wl_callback_listener sl_frame_callback_listener = {
    sl_frame_callback_done};

static void sl_host_callback_destroy(struct wl_resource* resource) {
  struct sl_host_callback* host = wl_resource_get_user_data(resource);

  wl_callback_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_host_surface_frame(struct wl_client* client,
                                  struct wl_resource* resource,
                                  uint32_t callback) {
  struct sl_host_surface* host = wl_resource_get_user_data(resource);
  struct sl_host_callback* host_callback;

  host_callback = malloc(sizeof(*host_callback));
  assert(host_callback);

  host_callback->resource =
      wl_resource_create(client, &wl_callback_interface, 1, callback);
  wl_resource_set_implementation(host_callback->resource, NULL, host_callback,
                                 sl_host_callback_destroy);
  host_callback->proxy = wl_surface_frame(host->proxy);
  wl_callback_set_user_data(host_callback->proxy, host_callback);
  wl_callback_add_listener(host_callback->proxy, &sl_frame_callback_listener,
                           host_callback);
}

static void sl_host_surface_set_opaque_region(
    struct wl_client* client,
    struct wl_resource* resource,
    struct wl_resource* region_resource) {
  struct sl_host_surface* host = wl_resource_get_user_data(resource);
  struct sl_host_region* host_region =
      region_resource ? wl_resource_get_user_data(region_resource) : NULL;

  wl_surface_set_opaque_region(host->proxy,
                               host_region ? host_region->proxy : NULL);
}

static void sl_host_surface_set_input_region(
    struct wl_client* client,
    struct wl_resource* resource,
    struct wl_resource* region_resource) {
  struct sl_host_surface* host = wl_resource_get_user_data(resource);
  struct sl_host_region* host_region =
      region_resource ? wl_resource_get_user_data(region_resource) : NULL;

  wl_surface_set_input_region(host->proxy,
                              host_region ? host_region->proxy : NULL);
}

static void sl_host_surface_commit(struct wl_client* client,
                                   struct wl_resource* resource) {
  struct sl_host_surface* host = wl_resource_get_user_data(resource);
  struct sl_viewport* viewport = NULL;
  struct sl_window* window;

  if (!wl_list_empty(&host->contents_viewport))
    viewport = wl_container_of(host->contents_viewport.next, viewport, link);

  if (host->contents_shm_mmap) {
    uint8_t* src_addr = host->contents_shm_mmap->addr;
    uint8_t* dst_addr = host->current_buffer->mmap->addr;
    size_t* src_offset = host->contents_shm_mmap->offset;
    size_t* dst_offset = host->current_buffer->mmap->offset;
    size_t* src_stride = host->contents_shm_mmap->stride;
    size_t* dst_stride = host->current_buffer->mmap->stride;
    size_t* y_ss = host->contents_shm_mmap->y_ss;
    size_t bpp = host->contents_shm_mmap->bpp;
    size_t num_planes = host->contents_shm_mmap->num_planes;
    double contents_scale_x = host->contents_scale;
    double contents_scale_y = host->contents_scale;
    double contents_offset_x = 0.0;
    double contents_offset_y = 0.0;
    pixman_box32_t* rect;
    int n;

    // Determine scale and offset for damage based on current viewport.
    if (viewport) {
      double contents_width = host->contents_width;
      double contents_height = host->contents_height;

      if (viewport->src_x >= 0 && viewport->src_y >= 0) {
        contents_offset_x = wl_fixed_to_double(viewport->src_x);
        contents_offset_y = wl_fixed_to_double(viewport->src_y);
      }

      if (viewport->dst_width > 0 && viewport->dst_height > 0) {
        contents_scale_x *= contents_width / viewport->dst_width;
        contents_scale_y *= contents_height / viewport->dst_height;

        // Take source rectangle into account when both destionation size and
        // source rectangle are set. If only source rectangle is set, then
        // it determines the surface size so it can be ignored.
        if (viewport->src_width >= 0 && viewport->src_height >= 0) {
          contents_scale_x *=
              wl_fixed_to_double(viewport->src_width) / contents_width;
          contents_scale_y *=
              wl_fixed_to_double(viewport->src_height) / contents_height;
        }
      }
    }

    if (host->current_buffer->mmap->begin_write)
      host->current_buffer->mmap->begin_write(host->current_buffer->mmap->fd);

    rect = pixman_region32_rectangles(&host->current_buffer->damage, &n);
    while (n--) {
      int32_t x1, y1, x2, y2;

      // Enclosing rect after applying scale and offset.
      x1 = rect->x1 * contents_scale_x + contents_offset_x;
      y1 = rect->y1 * contents_scale_y + contents_offset_y;
      x2 = rect->x2 * contents_scale_x + contents_offset_x + 0.5;
      y2 = rect->y2 * contents_scale_y + contents_offset_y + 0.5;

      x1 = MAX(0, x1);
      y1 = MAX(0, y1);
      x2 = MIN(host->contents_width, x2);
      y2 = MIN(host->contents_height, y2);

      if (x1 < x2 && y1 < y2) {
        size_t i;

        for (i = 0; i < num_planes; ++i) {
          uint8_t* src_base = src_addr + src_offset[i];
          uint8_t* dst_base = dst_addr + dst_offset[i];
          uint8_t* src = src_base + y1 * src_stride[i] + x1 * bpp;
          uint8_t* dst = dst_base + y1 * dst_stride[i] + x1 * bpp;
          int32_t width = x2 - x1;
          int32_t height = (y2 - y1) / y_ss[i];
          size_t bytes = width * bpp;

          while (height--) {
            memcpy(dst, src, bytes);
            dst += dst_stride[i];
            src += src_stride[i];
          }
        }
      }

      ++rect;
    }

    if (host->current_buffer->mmap->end_write)
      host->current_buffer->mmap->end_write(host->current_buffer->mmap->fd);

    pixman_region32_clear(&host->current_buffer->damage);

    wl_list_remove(&host->current_buffer->link);
    wl_list_insert(&host->busy_buffers, &host->current_buffer->link);
  }

  if (host->contents_width && host->contents_height) {
    double scale = host->ctx->scale * host->contents_scale;

    if (host->viewport) {
      int width = host->contents_width;
      int height = host->contents_height;

      // We need to take the client's viewport into account while still
      // making sure our scale is accounted for.
      if (viewport) {
        if (viewport->src_x >= 0 && viewport->src_y >= 0 &&
            viewport->src_width >= 0 && viewport->src_height >= 0) {
          wp_viewport_set_source(host->viewport, viewport->src_x,
                                 viewport->src_y, viewport->src_width,
                                 viewport->src_height);

          // If the source rectangle is set and the destination size is not
          // set, then src_width and src_height should be integers, and the
          // surface size becomes the source rectangle size.
          width = wl_fixed_to_int(viewport->src_width);
          height = wl_fixed_to_int(viewport->src_height);
        }

        // Use destination size as surface size when set.
        if (viewport->dst_width >= 0 && viewport->dst_height >= 0) {
          width = viewport->dst_width;
          height = viewport->dst_height;
        }
      }

      wp_viewport_set_destination(host->viewport, ceil(width / scale),
                                  ceil(height / scale));
    } else {
      wl_surface_set_buffer_scale(host->proxy, scale);
    }
  }

  // No need to defer client commits if surface has a role. E.g. is a cursor
  // or shell surface.
  if (host->has_role) {
    wl_surface_commit(host->proxy);

    // GTK determines the scale based on the output the surface has entered.
    // If the surface has not entered any output, then have it enter the
    // internal output. TODO(reveman): Remove this when surface-output tracking
    // has been implemented in Chrome.
    if (!host->has_output) {
      struct sl_host_output* output;

      wl_list_for_each(output, &host->ctx->host_outputs, link) {
        if (output->internal) {
          wl_surface_send_enter(host->resource, output->resource);
          host->has_output = 1;
          break;
        }
      }
    }
  } else {
    // Commit if surface is associated with a window. Otherwise, defer
    // commit until window is created.
    wl_list_for_each(window, &host->ctx->windows, link) {
      if (window->host_surface_id == wl_resource_get_id(resource)) {
        if (window->xdg_surface) {
          wl_surface_commit(host->proxy);
          if (host->contents_width && host->contents_height)
            window->realized = 1;
        }
        break;
      }
    }
  }

  if (host->contents_shm_mmap) {
    if (host->contents_shm_mmap->buffer_resource)
      wl_buffer_send_release(host->contents_shm_mmap->buffer_resource);
    sl_mmap_unref(host->contents_shm_mmap);
    host->contents_shm_mmap = NULL;
  }
}

static void sl_host_surface_set_buffer_transform(struct wl_client* client,
                                                 struct wl_resource* resource,
                                                 int32_t transform) {
  struct sl_host_surface* host = wl_resource_get_user_data(resource);

  wl_surface_set_buffer_transform(host->proxy, transform);
}

static void sl_host_surface_set_buffer_scale(struct wl_client* client,
                                             struct wl_resource* resource,
                                             int32_t scale) {
  struct sl_host_surface* host = wl_resource_get_user_data(resource);

  host->contents_scale = scale;
}

static void sl_host_surface_damage_buffer(struct wl_client* client,
                                          struct wl_resource* resource,
                                          int32_t x,
                                          int32_t y,
                                          int32_t width,
                                          int32_t height) {
  assert(0);
}

static const struct wl_surface_interface sl_surface_implementation = {
    sl_host_surface_destroy,
    sl_host_surface_attach,
    sl_host_surface_damage,
    sl_host_surface_frame,
    sl_host_surface_set_opaque_region,
    sl_host_surface_set_input_region,
    sl_host_surface_commit,
    sl_host_surface_set_buffer_transform,
    sl_host_surface_set_buffer_scale,
    sl_host_surface_damage_buffer};

static void sl_destroy_host_surface(struct wl_resource* resource) {
  struct sl_host_surface* host = wl_resource_get_user_data(resource);
  struct sl_window *window, *surface_window = NULL;
  struct sl_output_buffer* buffer;

  wl_list_for_each(window, &host->ctx->windows, link) {
    if (window->host_surface_id == wl_resource_get_id(resource)) {
      surface_window = window;
      break;
    }
  }

  if (surface_window) {
    surface_window->host_surface_id = 0;
    sl_window_update(surface_window);
  }

  if (host->contents_shm_mmap)
    sl_mmap_unref(host->contents_shm_mmap);

  while (!wl_list_empty(&host->released_buffers)) {
    buffer = wl_container_of(host->released_buffers.next, buffer, link);
    sl_output_buffer_destroy(buffer);
  }
  while (!wl_list_empty(&host->busy_buffers)) {
    buffer = wl_container_of(host->busy_buffers.next, buffer, link);
    sl_output_buffer_destroy(buffer);
  }
  while (!wl_list_empty(&host->contents_viewport))
    wl_list_remove(host->contents_viewport.next);

  if (host->viewport)
    wp_viewport_destroy(host->viewport);
  wl_surface_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_surface_enter(void* data,
                             struct wl_surface* surface,
                             struct wl_output* output) {
  struct sl_host_surface* host = wl_surface_get_user_data(surface);
  struct sl_host_output* host_output = wl_output_get_user_data(output);

  wl_surface_send_enter(host->resource, host_output->resource);
  host->has_output = 1;
}

static void sl_surface_leave(void* data,
                             struct wl_surface* surface,
                             struct wl_output* output) {
  struct sl_host_surface* host = wl_surface_get_user_data(surface);
  struct sl_host_output* host_output = wl_output_get_user_data(output);

  wl_surface_send_leave(host->resource, host_output->resource);
}

static const struct wl_surface_listener sl_surface_listener = {
    sl_surface_enter, sl_surface_leave};

static void sl_region_destroy(struct wl_client* client,
                              struct wl_resource* resource) {
  wl_resource_destroy(resource);
}

static void sl_region_add(struct wl_client* client,
                          struct wl_resource* resource,
                          int32_t x,
                          int32_t y,
                          int32_t width,
                          int32_t height) {
  struct sl_host_region* host = wl_resource_get_user_data(resource);
  double scale = host->ctx->scale;
  int32_t x1, y1, x2, y2;

  x1 = x / scale;
  y1 = y / scale;
  x2 = (x + width) / scale;
  y2 = (y + height) / scale;

  wl_region_add(host->proxy, x1, y1, x2 - x1, y2 - y1);
}

static void sl_region_subtract(struct wl_client* client,
                               struct wl_resource* resource,
                               int32_t x,
                               int32_t y,
                               int32_t width,
                               int32_t height) {
  struct sl_host_region* host = wl_resource_get_user_data(resource);
  double scale = host->ctx->scale;
  int32_t x1, y1, x2, y2;

  x1 = x / scale;
  y1 = y / scale;
  x2 = (x + width) / scale;
  y2 = (y + height) / scale;

  wl_region_subtract(host->proxy, x1, y1, x2 - x1, y2 - y1);
}

static const struct wl_region_interface sl_region_implementation = {
    sl_region_destroy, sl_region_add, sl_region_subtract};

static void sl_destroy_host_region(struct wl_resource* resource) {
  struct sl_host_region* host = wl_resource_get_user_data(resource);

  wl_region_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_compositor_create_host_surface(struct wl_client* client,
                                              struct wl_resource* resource,
                                              uint32_t id) {
  struct sl_host_compositor* host = wl_resource_get_user_data(resource);
  struct sl_host_surface* host_surface;
  struct sl_window *window, *unpaired_window = NULL;

  host_surface = malloc(sizeof(*host_surface));
  assert(host_surface);

  host_surface->ctx = host->compositor->ctx;
  host_surface->contents_width = 0;
  host_surface->contents_height = 0;
  host_surface->contents_scale = 1;
  wl_list_init(&host_surface->contents_viewport);
  host_surface->contents_shm_mmap = NULL;
  host_surface->has_role = 0;
  host_surface->has_output = 0;
  host_surface->last_event_serial = 0;
  host_surface->current_buffer = NULL;
  wl_list_init(&host_surface->released_buffers);
  wl_list_init(&host_surface->busy_buffers);
  host_surface->resource = wl_resource_create(
      client, &wl_surface_interface, wl_resource_get_version(resource), id);
  wl_resource_set_implementation(host_surface->resource,
                                 &sl_surface_implementation, host_surface,
                                 sl_destroy_host_surface);
  host_surface->proxy = wl_compositor_create_surface(host->proxy);
  wl_surface_set_user_data(host_surface->proxy, host_surface);
  wl_surface_add_listener(host_surface->proxy, &sl_surface_listener,
                          host_surface);
  host_surface->viewport = NULL;
  if (host_surface->ctx->viewporter) {
    host_surface->viewport = wp_viewporter_get_viewport(
        host_surface->ctx->viewporter->internal, host_surface->proxy);
  }

  wl_list_for_each(window, &host->compositor->ctx->unpaired_windows, link) {
    if (window->host_surface_id == id) {
      unpaired_window = window;
      break;
    }
  }

  if (unpaired_window)
    sl_window_update(window);
}

static void sl_compositor_create_host_region(struct wl_client* client,
                                             struct wl_resource* resource,
                                             uint32_t id) {
  struct sl_host_compositor* host = wl_resource_get_user_data(resource);
  struct sl_host_region* host_region;

  host_region = malloc(sizeof(*host_region));
  assert(host_region);

  host_region->ctx = host->compositor->ctx;
  host_region->resource = wl_resource_create(
      client, &wl_region_interface, wl_resource_get_version(resource), id);
  wl_resource_set_implementation(host_region->resource,
                                 &sl_region_implementation, host_region,
                                 sl_destroy_host_region);
  host_region->proxy = wl_compositor_create_region(host->proxy);
  wl_region_set_user_data(host_region->proxy, host_region);
}

static const struct wl_compositor_interface sl_compositor_implementation = {
    sl_compositor_create_host_surface, sl_compositor_create_host_region};

static void sl_destroy_host_compositor(struct wl_resource* resource) {
  struct sl_host_compositor* host = wl_resource_get_user_data(resource);

  wl_compositor_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_bind_host_compositor(struct wl_client* client,
                                    void* data,
                                    uint32_t version,
                                    uint32_t id) {
  struct sl_context* ctx = (struct sl_context*)data;
  struct sl_host_compositor* host;

  host = malloc(sizeof(*host));
  assert(host);
  host->compositor = ctx->compositor;
  host->resource =
      wl_resource_create(client, &wl_compositor_interface,
                         MIN(version, ctx->compositor->version), id);
  wl_resource_set_implementation(host->resource, &sl_compositor_implementation,
                                 host, sl_destroy_host_compositor);
  host->proxy = wl_registry_bind(wl_display_get_registry(ctx->display),
                                 ctx->compositor->id, &wl_compositor_interface,
                                 ctx->compositor->version);
  wl_compositor_set_user_data(host->proxy, host);
}

struct sl_global* sl_compositor_global_create(struct sl_context* ctx) {
  return sl_global_create(ctx, &wl_compositor_interface,
                          ctx->compositor->version, ctx,
                          sl_bind_host_compositor);
}
