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
    if ((v3 == 6)) {
        {
            v1 = 1;
            v2 = 1;
            v1 = (v2 * v1);
            ((v2 = (v2 + 1)) <= v0);
            /* break */;
            v1 = 1;
            if ((v0 < 1)) {
                /* break */;
            } else {
                v3 = 6;
            }
            /* loop: shape$continue$3 */ while (1) {
                {
                    v1 = (v2 * v1);
                    ((v2 = (v2 + 1)) <= v0);
                    v1 = (v2 * v1);
                    ((v2 = (v2 + 1)) <= v0);
                    /* break */;
                    if ((v0 < 1)) {
                        /* break */;
                    } else {
                        /* break */;
                    }
                    v1 = 1;
                    v2 = 1;
                    v1 = (v2 * v1);
                    ((v2 = (v2 + 1)) <= v0);
                }
            }
            while (1) {
                /* unsupported expr: LocalGet { id: Id(1:0) } */;
            }
        }
    }
}