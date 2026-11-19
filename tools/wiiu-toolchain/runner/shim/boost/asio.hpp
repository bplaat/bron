#pragma once
// Headless shim: the OS emulation doesn't use asio, only the socket types it brought along
#include <arpa/inet.h>
#include <netinet/in.h>
#include <sys/socket.h>
