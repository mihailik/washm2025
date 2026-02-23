// Decompiled to C from WebAssembly

#include <stdint.h>
#include <stdbool.h>

void memcopy(int32_t v0, int32_t v1, int32_t v2) {
    int32_t v3 = 0;
    {
        if ((v3 == 4)) {
            {
                memory_copy(v1, v0, v2);
                /* break */;
                v3 = 4;
                memory_copy(v1, v0, v2);
            }
        }
        while (1) {
            /* nop */;
        }
    }
}