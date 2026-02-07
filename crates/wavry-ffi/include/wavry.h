#ifndef WAVRY_H
#define WAVRY_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    uint16_t width;
    uint16_t height;
    uint16_t fps;
    uint32_t bitrate_kbps;
    uint32_t keyframe_interval_ms;
    uint32_t display_id; // u32::MAX for None
} WavryHostConfig;

typedef struct {
    bool connected;
    uint32_t fps;
    uint32_t rtt_ms;
    uint32_t bitrate_kbps;
    uint64_t frames_encoded;
    uint64_t frames_decoded;
} WavryStats;

// Lifecycle
void wavry_init(void);
const char *wavry_version(void);
int32_t wavry_stop(void);

// Identity
int32_t wavry_init_identity(const char *storage_path);
int32_t wavry_get_public_key(uint8_t *out_buffer_32);

// Session Control
int32_t wavry_start_host(uint16_t port);
int32_t wavry_start_host_with_config(uint16_t port, const WavryHostConfig *config);
int32_t wavry_start_client(const char *host_ip, uint16_t port);

// Signaling / Cloud
int32_t wavry_connect_signaling(const char *token);
int32_t wavry_connect_signaling_with_url(const char *url, const char *token);
int32_t wavry_send_connect_request(const char *target_username);

// Monitoring & Stats
int32_t wavry_get_stats(WavryStats *out);
int32_t wavry_copy_last_error(char *out_buffer, uint32_t out_buffer_len);
int32_t wavry_copy_last_cloud_status(char *out_buffer, uint32_t out_buffer_len);

// Media & Input
int32_t wavry_init_renderer(void *layer_ptr);
int32_t wavry_init_injector(uint32_t width, uint32_t height);
int32_t wavry_test_input_injection(void);

#ifdef __cplusplus
}
#endif

#endif