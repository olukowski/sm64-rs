/* Original capture-only Mupen64Plus input adapter, not game code.
 * ABI reference: Mupen64Plus 2.6.0 src/api/m64p_{common,plugin,types}.h.
 * Those upstream headers are acquired separately and are not bundled here.
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */
#define M64P_PLUGIN_PROTOTYPES
#include "m64p_common.h"
#include "m64p_plugin.h"
#include <stdint.h>
#include <stdatomic.h>
#include <string.h>

#if !defined(__BYTE_ORDER__) || __BYTE_ORDER__ != __ORDER_LITTLE_ENDIAN__
#error "The capture adapter is validated only on little-endian hosts"
#endif
_Static_assert(sizeof(BUTTONS) == 4, "unexpected controller ABI");

static _Atomic uint32_t packet;
static _Atomic uint64_t polls;
/* Optional synchronous poll hook; configured before execution, never concurrently. */
static void (*poll_hook)(uint64_t);

EXPORT void CALL CaptureSetInput(uint16_t buttons, int8_t x, int8_t y) {
    /* Game-visible button halfword, then signed axes, in wire byte order. */
    unsigned char bytes[4] = {(unsigned char)(buttons >> 8),
        (unsigned char)buttons, (unsigned char)x, (unsigned char)y};
    uint32_t value;
    memcpy(&value, bytes, sizeof(value));
    atomic_store(&packet, value);
}
EXPORT uint64_t CALL CapturePollCount(void) { return atomic_load(&polls); }
EXPORT void CALL CaptureSetPollHook(void (*hook)(uint64_t)) { poll_hook = hook; }

EXPORT m64p_error CALL PluginGetVersion(m64p_plugin_type *type, int *version,
        int *api, const char **name, int *capabilities) {
    if (type) *type = M64PLUGIN_INPUT;
    if (version) *version = 0x010000;
    if (api) *api = 0x020100;
    if (name) *name = "sm64-rs explicit input capture bridge";
    if (capabilities) *capabilities = 0;
    return M64ERR_SUCCESS;
}
EXPORT m64p_error CALL PluginStartup(m64p_dynlib_handle core, void *context,
        void (*debug)(void *, int, const char *)) {
    (void)core; (void)context; (void)debug;
    atomic_store(&packet, 0); atomic_store(&polls, 0); poll_hook = NULL;
    return M64ERR_SUCCESS;
}
EXPORT m64p_error CALL PluginShutdown(void) { poll_hook = NULL; return M64ERR_SUCCESS; }
EXPORT void CALL InitiateControllers(CONTROL_INFO info) {
    for (int i = 0; i < 4; ++i) {
        info.Controls[i].Present = (i == 0);
        info.Controls[i].RawData = 0;
        info.Controls[i].Plugin = PLUGIN_NONE;
        info.Controls[i].Type = CONT_TYPE_STANDARD;
    }
}
EXPORT int CALL RomOpen(void) { return 1; }
EXPORT void CALL RomClosed(void) {}
EXPORT void CALL GetKeys(int controller, BUTTONS *keys) {
    if (controller == 0) {
        uint64_t index = atomic_fetch_add(&polls, 1);
        if (poll_hook) poll_hook(index);
    }
    if (keys) keys->Value = controller == 0 ? atomic_load(&packet) : 0;
}
EXPORT void CALL ControllerCommand(int controller, unsigned char *command) {
    (void)controller; (void)command;
}
EXPORT void CALL ReadController(int controller, unsigned char *command) {
    (void)controller; (void)command;
}
EXPORT void CALL SDL_KeyDown(int mod, int key) { (void)mod; (void)key; }
EXPORT void CALL SDL_KeyUp(int mod, int key) { (void)mod; (void)key; }
