/* elle.h — C embedding interface for Elle */
#ifndef ELLE_H
#define ELLE_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque context handle */
typedef void *elle_ctx_t;

/* Lifecycle */
elle_ctx_t elle_init(void);
void       elle_destroy(elle_ctx_t ctx);

/* Eval — returns 0 on success, -1 on error */
int elle_eval(elle_ctx_t ctx, const uint8_t *src, size_t len);

/* Result access */
bool elle_result_int(elle_ctx_t ctx, int64_t *out);

/* Value constructors (for use in custom primitives) */
typedef struct { uint64_t bits[2]; } elle_value_t;

elle_value_t elle_make_int(int64_t n);
elle_value_t elle_make_nil(void);

#ifdef __cplusplus
}
#endif

#endif /* ELLE_H */
