// Decompiled to C from WebAssembly

#include <stdint.h>
#include <stdbool.h>

int32_t add(int32_t v0, int32_t v1) {
    int32_t v2 = 0;
    return ((v3 = v1) + (v4 = v0));
}

int32_t factorial(int32_t v0) {
    int32_t v1 = 0;
    int32_t v2 = 0;
    int32_t v3 = 0;
    if (((v4 = v3) == (v5 = 6))) {
        {
            v1 = v6;
            v2 = v13;
            v1 = ((v7 = (v17 = v12)) * (v8 = v14));
            ((v2 = (v9 = ((v12 = v17) + (v13 = (v18 = v13))))) <= (v10 = v15));
            /* break */;
            v1 = v6;
            if ((v11 = (v15 < v13))) {
                /* break */;
            } else {
                v3 = 6;
            }
            /* loop: shape$continue$3 */ while (1) {
                {
                    v1 = ((v7 = (v17 = v12)) * (v8 = v14));
                    ((v2 = (v9 = ((v12 = v17) + (v13 = (v18 = v13))))) <= (v10 = v15));
                    v1 = ((v7 = (v17 = v12)) * (v8 = v14));
                    ((v2 = (v9 = ((v12 = v17) + (v13 = (v18 = v13))))) <= (v10 = v15));
                    /* break */;
                    if ((v16 = (v15 < v13))) {
                        /* break */;
                    } else {
                        /* break */;
                    }
                    v1 = v6;
                    v2 = v13;
                    v1 = ((v7 = (v17 = v12)) * (v8 = v14));
                    ((v2 = (v9 = ((v12 = v17) + (v13 = (v18 = v13))))) <= (v10 = v15));
                }
            }
            while (1) {
                /* unsupported expr: LocalGet { id: Id(8:0) } */;
            }
        }
    }
}