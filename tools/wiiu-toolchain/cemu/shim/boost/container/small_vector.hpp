#pragma once
// Headless shim: a small vector is a vector, the OS emulation doesn't depend on its inline storage
#include <cstddef>
#include <vector>
namespace boost::container {
template <typename T, std::size_t N, typename... Options>
using small_vector = std::vector<T>;
}  // namespace boost::container
