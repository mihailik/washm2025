// Decompiled to C from WebAssembly

#include <stdint.h>
#include <stdbool.h>

int32_t sw(int32_t v0) {
    int32_t v1 = 0;
    int32_t v2 = 0;
    {
        if (((v3 = v2) == (v4 = 4))) {
            {
                v1 = ((v5 << v6)) /* load */;
                if ((v7 = ((v9 = v5) > (v10 = v4)))) {
                    /* break */;
                } else {
                    /* break */;
                }
                v1 = (v8 = 99);
                if (((v9 = v5) > (v10 = v4))) {
                    /* break */;
                } else {
                    v2 = v4;
                }
                v1 = ((v5 << v6)) /* load */;
                /* break */;
            }
        }
        return (v11 = v1);
    }
}