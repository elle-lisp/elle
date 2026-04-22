/* host.c — C host demo for Elle embedding */

#include <stdio.h>
#include <string.h>
#include "elle.h"

int main(void) {
    elle_ctx_t ctx = elle_init();
    if (!ctx) {
        fprintf(stderr, "elle_init failed\n");
        return 1;
    }

    const char *src = "(+ 100 200 300)";
    int rc = elle_eval(ctx, (const uint8_t *)src, strlen(src));
    if (rc != 0) {
        fprintf(stderr, "elle_eval failed\n");
        elle_destroy(ctx);
        return 1;
    }

    int64_t result;
    if (elle_result_int(ctx, &result)) {
        printf("Elle returned: %ld\n", result);
    } else {
        printf("Result is not an integer\n");
    }

    elle_destroy(ctx);
    return 0;
}
