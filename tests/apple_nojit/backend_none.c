/* Default backend: no interpreter linked. Refuses every case cleanly, so the report lists every
 * family as unsupported. This proves the harness and report only, not interpreter coverage. */
#include "nojit_abi.h"
int nojit_run(const uint8_t *c, size_t l, const nojit_state *i, nojit_state *o, int *f) {
    (void)c; (void)l; (void)i; (void)o; (void)f;
    return NOJIT_UNSUPPORTED;
}
const char *nojit_backend_name(void) { return "none"; }
