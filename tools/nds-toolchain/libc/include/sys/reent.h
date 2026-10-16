#pragma once

// What newlib passes to the devoptab functions: they report their error in it and get the
// deviceData of their devoptab
struct _reent {
    int _errno;
    void* deviceData;
};
