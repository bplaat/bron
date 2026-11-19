#pragma once
// Headless shim: Cemu's platform header includes X11 on Linux for its GUI, whose window handles name
// these types
typedef struct _XDisplay Display;
typedef unsigned long Window;
