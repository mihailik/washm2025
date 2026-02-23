// Decompiled to C from WebAssembly

#include <stdint.h>
#include <stdbool.h>

int32_t add(int32_t acc, int32_t acc1) {
    int32_t v2 = 0;
    return (acc1 + acc);
}

int32_t factorial(int32_t v0) {
    int32_t v1 = 0;
    int32_t v2 = 0;
    int32_t v3 = 0;
    {
        v1 = 1;
        if ((v0 < 1)) {
            goto block_break_2_end;
        } else {
            {
                v3 = 8;
                v1 = 1;
                v2 = 1;
                /* loop: shape$continue$4 */ while (1) {
                    {
                        v1 = (v2 * v1);
                        if (!(((v2 = (v2 + 1)) <= v0))) {
                            goto block_break_9_end;
                        }
                    }
                }
                /* block: block$break$9 */ {
                }
block_break_9_end: ;
            }
        }
        /* block: block$break$2 */ {
            return v1;
        }
block_break_2_end: ;
    }
}