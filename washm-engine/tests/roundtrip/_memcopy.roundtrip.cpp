// Decompiled to C from WebAssembly

#include <stdint.h>
#include <stdbool.h>

void memcopy(int32_t v0, int32_t v1, int32_t v2) {
    int32_t v3 = 0;
    if (((v4 = 4) == (v5 = 4)))     {
        memory_copy(v6, v7, v8);
        if ((v9 = unary((v10 = v8))))         /* break */;
        else         /* break */;
        if (unary((v10 = v8)))         /* break */;
        else         memory_copy(v6, v7, v8);
        /* break */;
    }
}

