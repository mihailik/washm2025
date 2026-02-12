// Decompiled to C from WebAssembly

#include <stdint.h>
#include <stdbool.h>

void memcopy(int32_t v0, int32_t p1, int32_t p2) {
    int32_t v1 = 0;
    {
        if (!(v0)) {
            goto block_break_1_end;
        }
        memory_copy(v0, v0, v0);
        /* block: block$break$1 */ {
        }
        block_break_1_end: ;
    }
}