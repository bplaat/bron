/* IUnknown for TinyCC: the base of every COM interface, like DirectDraw and Direct3D 8. TinyCC compiles C, so the
   interfaces are structs with a vtable and the IUnknown_* macros call through it. */

#ifndef __unknwn_h__
#define __unknwn_h__

#include <windows.h>
#include <basetyps.h>

#undef INTERFACE
#define INTERFACE IUnknown
DECLARE_INTERFACE(IUnknown) {
    STDMETHOD(QueryInterface)(THIS_ REFIID riid, void **ppvObject) PURE;
    STDMETHOD_(ULONG, AddRef)(THIS) PURE;
    STDMETHOD_(ULONG, Release)(THIS) PURE;
};
#undef INTERFACE

typedef IUnknown *LPUNKNOWN;

#define IUnknown_QueryInterface(This, riid, ppvObject) (This)->lpVtbl->QueryInterface(This, riid, ppvObject)
#define IUnknown_AddRef(This) (This)->lpVtbl->AddRef(This)
#define IUnknown_Release(This) (This)->lpVtbl->Release(This)

/* Defined where INITGUID is defined before including, TinyCC has no uuid library */
DEFINE_GUID(IID_IUnknown, 0x00000000, 0x0000, 0x0000, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46);

#endif
