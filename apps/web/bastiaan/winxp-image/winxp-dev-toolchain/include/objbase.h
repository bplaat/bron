/* The COM base for TinyCC: the interface macros, IUnknown and the COM library of ole32.dll (link with -lole32). The
   mingw-w64 headers of DirectDraw and Direct3D 8 include it. */

#ifndef _OBJBASE_H_
#define _OBJBASE_H_

#include <windows.h>
#include <basetyps.h>
#include <unknwn.h>

/* C has no __uuidof, the interface ID stays a DEFINE_GUID */
#define DECLARE_INTERFACE_IID(iface, iid) DECLARE_INTERFACE(iface)
#define DECLARE_INTERFACE_IID_(iface, baseiface, iid) DECLARE_INTERFACE_(iface, baseiface)

#define WINOLEAPI EXTERN_C HRESULT WINAPI
#define WINOLEAPI_(type) EXTERN_C type WINAPI

/* A stream of bytes, like GDI+ reads images from. Used through IUnknown_Release and the functions that take one. */
typedef struct IStream IStream;
typedef IStream *LPSTREAM;

typedef WCHAR OLECHAR;
typedef OLECHAR *LPOLESTR;
typedef const OLECHAR *LPCOLESTR;

typedef enum tagCOINIT {
    COINIT_APARTMENTTHREADED = 0x2,
    COINIT_MULTITHREADED = 0x0,
    COINIT_DISABLE_OLE1DDE = 0x4,
    COINIT_SPEED_OVER_MEMORY = 0x8
} COINIT;

typedef enum tagCLSCTX {
    CLSCTX_INPROC_SERVER = 0x1,
    CLSCTX_INPROC_HANDLER = 0x2,
    CLSCTX_LOCAL_SERVER = 0x4,
    CLSCTX_INPROC_SERVER16 = 0x8,
    CLSCTX_REMOTE_SERVER = 0x10,
    CLSCTX_INPROC_HANDLER16 = 0x20,
    CLSCTX_NO_CODE_DOWNLOAD = 0x400,
    CLSCTX_NO_CUSTOM_MARSHAL = 0x1000,
    CLSCTX_ENABLE_CODE_DOWNLOAD = 0x2000,
    CLSCTX_NO_FAILURE_LOG = 0x4000,
    CLSCTX_DISABLE_AAA = 0x8000,
    CLSCTX_ENABLE_AAA = 0x10000,
    CLSCTX_FROM_DEFAULT_CONTEXT = 0x20000
} CLSCTX;

#define CLSCTX_INPROC (CLSCTX_INPROC_SERVER | CLSCTX_INPROC_HANDLER)
#define CLSCTX_SERVER (CLSCTX_INPROC_SERVER | CLSCTX_LOCAL_SERVER | CLSCTX_REMOTE_SERVER)
#define CLSCTX_ALL (CLSCTX_INPROC_SERVER | CLSCTX_INPROC_HANDLER | CLSCTX_LOCAL_SERVER | CLSCTX_REMOTE_SERVER)

WINOLEAPI CoInitialize(LPVOID pvReserved);
WINOLEAPI CoInitializeEx(LPVOID pvReserved, DWORD dwCoInit);
WINOLEAPI_(void) CoUninitialize(void);
WINOLEAPI CoCreateInstance(REFCLSID rclsid, LPUNKNOWN pUnkOuter, DWORD dwClsContext, REFIID riid, LPVOID *ppv);
WINOLEAPI_(LPVOID) CoTaskMemAlloc(SIZE_T cb);
WINOLEAPI_(LPVOID) CoTaskMemRealloc(LPVOID pv, SIZE_T cb);
WINOLEAPI_(void) CoTaskMemFree(LPVOID pv);
WINOLEAPI CLSIDFromString(LPCOLESTR lpsz, LPCLSID pclsid);
WINOLEAPI_(int) StringFromGUID2(REFGUID rguid, LPOLESTR lpsz, int cchMax);
WINOLEAPI CreateStreamOnHGlobal(HGLOBAL hGlobal, WINBOOL fDeleteOnRelease, LPSTREAM *ppstm);

#endif
