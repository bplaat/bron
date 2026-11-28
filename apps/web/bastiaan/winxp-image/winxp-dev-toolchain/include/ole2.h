/* OLE for TinyCC: the COM base and OLE initialization of ole32.dll (link with -lole32) */

#ifndef _OLE2_H_
#define _OLE2_H_

#include <objbase.h>

WINOLEAPI OleInitialize(LPVOID pvReserved);
WINOLEAPI_(void) OleUninitialize(void);

#endif
