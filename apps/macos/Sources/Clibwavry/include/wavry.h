#ifndef WAVRY_H
#define WAVRY_H

#include <stdint.h>

int wavry_init(void);
const char *wavry_version(void);
int wavry_connect(void);
int wavry_init_renderer(void *layer_ptr);
int wavry_init_injector(unsigned int width, unsigned int height);
int wavry_test_input_injection(void);

#endif
