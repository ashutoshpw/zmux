#include <stdbool.h>

bool zmux_auth_callback_install(void);
bool zmux_auth_callback_has_completion(void);
bool zmux_auth_callback_invoke_failure(void);
void zmux_auth_callback_reset(void);
