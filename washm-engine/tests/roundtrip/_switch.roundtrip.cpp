// Decompiled to C from WebAssembly

#include <stdint.h>
#include <stdbool.h>

void side_effect(int32_t p0); // import from env.side_effect

int32_t sw(int32_t v0) {
    int32_t v1 = 0;
    int32_t v2 = 0;
    {
        side_effect(((v1 = (v0 < 3)) ? v0 : 3));
        return (v1 ? ((v0 * 10) + 10) : 999);
    }
}