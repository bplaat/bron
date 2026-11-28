/* libdxguid.a: the interface and class IDs of DirectDraw and Direct3D 8, so programs need no INITGUID. Weak, a
   program that defines INITGUID itself still links. */

#define INITGUID
#define DECLSPEC_SELECTANY __attribute__((weak))
#include <ddraw.h>
#include <d3d8.h>
