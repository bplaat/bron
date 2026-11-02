#pragma once
// Headless shim: Cemu's network headers include curl for requests the OS emulation never makes, so
// only the declarations its inline functions name
enum CURLoption {
    CURLOPT_USERAGENT = 10018,
};
typedef int CURLFORMcode;
int curl_easy_setopt(void* curl, CURLoption option, ...);
