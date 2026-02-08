#ifndef WAVRY_H
#define WAVRY_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

int wavry_init(void);
int wavry_android_init(void *vm, void *context);
const char *wavry_version(void);

// Session Management
int wavry_start_host(uint16_t port);
typedef struct {
  uint16_t width;
  uint16_t height;
  uint16_t fps;
  uint32_t bitrate_kbps;
  uint32_t keyframe_interval_ms;
  uint32_t display_id;
} WavryHostConfig;
int wavry_start_host_with_config(uint16_t port, const WavryHostConfig *config);
int wavry_start_client(const char *host_ip, uint16_t port);
int wavry_stop(void);

// Identity Management
int32_t wavry_init_identity(const char *storage_path);
int32_t wavry_get_public_key(uint8_t *out_buffer_32);
int32_t wavry_connect_signaling(const char *token);
int32_t wavry_connect_signaling_with_url(const char *url, const char *token);
int32_t wavry_send_connect_request(const char *target_username);

// Renderer & Injector
int wavry_init_renderer(void *layer_ptr);
int wavry_init_injector(unsigned int width, unsigned int height);
int wavry_test_input_injection(void);

// Stats
typedef struct {
  int32_t connected;
  uint32_t fps;
  uint32_t rtt_ms;
  uint32_t bitrate_kbps;
  uint64_t frames_encoded;
  uint64_t frames_decoded;
} WavryStats;

int wavry_get_stats(WavryStats *out);
int wavry_copy_last_error(char *out_buffer, uint32_t out_buffer_len);
int wavry_copy_last_cloud_status(char *out_buffer, uint32_t out_buffer_len);

#ifdef __cplusplus
}
#endif

#endif
