/* libuuid.a: the interface IDs of the COM base, so programs need no INITGUID. Weak, a program that defines
   INITGUID itself still links. */

#define INITGUID
#define DECLSPEC_SELECTANY __attribute__((weak))
#include <objbase.h>
