// No include guard, the standard wants assert to follow NDEBUG at every include

#undef assert
#ifdef NDEBUG
#define assert(x) ((void)0)
#else
// Like newlib's, a program can show failed assertions with its own __assert_func
void __assert_func(const char* file, int line, const char* function, const char* expression) __attribute__((noreturn));
#define assert(x) ((x) ? (void)0 : __assert_func(__FILE__, __LINE__, __func__, #x))
#endif
