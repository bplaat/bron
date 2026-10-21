# cContinue Transpiler

_A transpiler that translates an OOP extension for the C programming language back to C._

I like C and I like C++, both are powerful languages in their own right. But C++ is quite complicated and sometimes I just want to create some classes with inheritance in my C project. So I've created this weird hacky Rust transpiler that translates a C++ like syntax with a Java like class system back to C code 🤓.

## Usage

```sh
cargo install --path .
ccc [options] <file.cc>
```

| Flag        | Description                                    |
| ----------- | ---------------------------------------------- |
| `-o <file>` | Output file                                    |
| `-I <path>` | Add include search path                        |
| `-S`        | Only run the transpile step (emit `.c` source) |
| `-c`        | Only transpile and compile (emit `.o` object)  |
| `-r`        | Run the linked binary after building           |

`ccc` uses the platform C compiler selected by the Rust `cc` crate (for example,
MSVC on Windows and GCC on Linux). Set `CC` to select another C compiler, `LD`
to select the linker used by GNU-like compilers, `CFLAGS` to add compile flags,
and `LDFLAGS` to add linker flags.

## Syntax

### Classes

Classes implicitly extend `Object` (heap-allocated, reference-counted). Methods are declared in the class body and implemented with `ClassName::method()`. Use `this` inside methods, call parent methods with `ParentClass::method()`, and use `Self*` as return type when returning the current instance:

```cpp
class Person {
    @get @init(strdup) @deinit char* name;
    @prop @init i32 age;
    void greet();
};

void Person::greet() {
    printf("Hello %s, you are %d years old!\n", this->name, this->age);
}

int main(void) {
    Person* person = person_new("Bastiaan", 21);
    person_greet(person);
    person_free(person);
}
```

### Field attributes

Attributes before a field declaration auto-generate boilerplate:

| Attribute     | Effect                                                    |
| ------------- | --------------------------------------------------------- |
| `@get`        | Generate a `get_<field>()` getter                         |
| `@set`        | Generate a `set_<field>()` setter                         |
| `@prop`       | Alias for `@get` + `@set`                                 |
| `@init`       | Pass field as argument to `_new()`; assign directly       |
| `@init(fn)`   | Same but wrap the argument with `fn(arg)` (e.g. `strdup`) |
| `@deinit`     | Free field in `deinit`; calls `free()` by default         |
| `@deinit(fn)` | Same but call `fn(field)` instead of `free()`             |

### Inheritance & virtual methods

A class can extend **one** parent with `: Parent`. Mark methods `virtual` for vtable dispatch. A class with `virtual method = 0` is abstract:

```cpp
class Animal {
    @get @init(strdup) @deinit char* name;
    virtual void speak() = 0;
};

class Dog : Animal {
    virtual void speak();
};
void Dog::speak() {
    printf("Dog %s barks!\n", this->name);
}
```

### Static methods

Mark methods `static` to create class-level functions that don't take a `this` parameter. Static methods generate regular C functions without vtable dispatch:

```cpp
class Math {
    static i32 add(i32 a, i32 b);
    static i32 multiply(i32 a, i32 b);
};
i32 Math::add(i32 a, i32 b) { return a + b; }
i32 Math::multiply(i32 a, i32 b) { return a * b; }

i32 result = math_add(5, 3);
```

Note: Constructor methods (`ClassName_new()`) are generated as static methods internally.

### Interfaces

Interfaces are declared as `class IFoo` (name starts with `I` + uppercase). Implement with `: IFace`. Methods can have defaults with `IFoo::method()`. Dispatch via `cast<IFoo>(obj)` (fat pointer). Interfaces can extend other interfaces (multi-parent).

```cpp
class IEquatable {
    virtual bool equals(Object* other) = 0;
};

class IComparable : IEquatable {
    i32 compare(Object* other);
    bool less_than(Object* other);
    bool greater_than(Object* other);
};
bool IComparable::less_than(Object* other) { return compare(this, other) < 0; }
bool IComparable::greater_than(Object* other) { return compare(this, other) > 0; }

class Number : IComparable {
    @get @init i32 value;

    virtual bool equals(Object* other);
    virtual i32 compare(Object* other);
};

Number* n = number_new(42);
IComparable c = cast<IComparable>(n);
IEquatable  e = cast<IEquatable>(n);
```

Generated dispatch macros use the snake_case of the interface name:

- `IComparable` → `i_comparable_less_than(c, other)`
- `IHashable` → `i_hashable_hash(h)`

#### Auto interfaces

An interface with **no own methods** (only inherited from parents) is an _auto interface_. Any class that implements all of its parent interfaces automatically implements the auto interface - no explicit declaration needed. This mirrors Rust's auto traits:

```cpp
class IEquatable { bool equals(Object* other); };
class IHashable  { u32 hash(); };
class IKeyable : IEquatable, IHashable {};  // auto interface - zero own methods

class Point : IEquatable, IHashable {       // automatically implements IKeyable too
    @init i32 x;
    @init i32 y;
    virtual bool equals(Object* other);
    virtual u32  hash();
};

IKeyable k = cast<IKeyable>(point_new(1, 2));  // works without `: IKeyable` on Point
```

### Literals

`@"..."`, `@true`/`@false`, `@<int>`, and `@<float>` are sugar for the corresponding boxed-type constructors:

```cpp
String* s = @"hello";  // → string_new("hello")
Bool*   b = @true;     // → bool_new(true)
Int*    i = @42;       // → int_new(42)
Float*  f = @3.14;     // → float_new(3.14)
```

### for-in loop

Iterate over any `IIterable` (e.g. `List`) with sugar syntax:

```cpp
for (Person* person in persons) {
    person_greet(person);
}
```

Transpiles to a scoped block that calls `iterator()`, loops with `has_next()`/`next()`,
and frees the iterator after. Nested loops are fully supported.
Regular C `for` loops (with semicolons) are passed through unchanged.

### Type checks

`instanceof<Type>(expr)` returns a `bool`. Checks exact class for classes, interface slot for interfaces:

```cpp
if (instanceof<IHashable>(obj)) { ... }
if (instanceof<Dog>(obj)) { ... }
```

## Standard library

Include stdlib classes with `#include <ClassName.hh>`.

### `Object` - base class

Every class implicitly extends `Object`. Provides reference counting:

```c
Object* object_ref(Object* obj)   // Increment reference count; returns obj
void    object_free(Object* obj)  // Decrement reference count; frees when it reaches zero
```

### Interfaces

| Interface    | Methods                            | Notes                         |
| ------------ | ---------------------------------- | ----------------------------- |
| `IEquatable` | `equals(Object*)`                  | Value equality                |
| `IHashable`  | `hash()` → `u32`                   | Hash code                     |
| `IKeyable`   | _(extends both)_                   | Suitable as a `Map`/`Set` key |
| `IIterator`  | `has_next()`, `next()` → `Object*` | One-pass forward iterator     |
| `IIterable`  | `iterator()` → `IIterator`         | Any iterable collection       |

### `Bool` - heap boolean

```c
Bool* bool_new(bool value)        // Create a new Bool
void  bool_free(Bool* b)          // Free the Bool
bool  bool_get_value(Bool* b)     // Get the raw bool value
```

### `Int` - heap integer

```c
Int* int_new(i64 value)          // Create a new Int
void int_free(Int* i)            // Free the Int
i64  int_get_value(Int* i)       // Get the raw i64 value
```

### `Float` - heap float

```c
Float* float_new(f64 value)      // Create a new Float
void  float_free(Float* f)       // Free the Float
f64   float_get_value(Float* f)  // Get the raw f64 value
```

### `String` - heap string

Implements `IKeyable`. Stores an owned copy of the string.

```c
String* string_new(char* cstr)                              // Create (copies cstr with strdup)
void    string_free(String* s)                              // Free
char*   string_get_cstr(String* s)                          // Raw char* pointer
usize   string_get_length(String* s)                        // Length in bytes
bool    string_equals(String* s, Object* other)             // Content equality
u32     string_hash(String* s)                              // FNV-1a hash
bool    string_contains(String* s, char* substr)            // Substring test
bool    string_starts_with(String* s, char* prefix)         // Prefix test
bool    string_ends_with(String* s, char* suffix)           // Suffix test
String* string_to_upper(String* s)                          // New uppercase copy
String* string_to_lower(String* s)                          // New lowercase copy
String* string_trim(String* s)                              // New copy with whitespace stripped
i32     string_index_of(String* s, char* substr)            // First index; -1 if absent
String* string_substring(String* s, usize start, usize len) // New substring copy
```

### `StringBuilder` - mutable string builder

```cpp
#include <StringBuilder.hh>
```

```c
StringBuilder* string_builder_new()                          // Create empty builder
void           string_builder_free(StringBuilder* sb)        // Free
void           string_builder_append_cstr(StringBuilder* sb, char* s)    // Append C string
void           string_builder_append_char(StringBuilder* sb, char c)     // Append single char
void           string_builder_append_string(StringBuilder* sb, String* s) // Append String
String*        string_builder_build(StringBuilder* sb)       // Build → new String*
usize          string_builder_get_length(StringBuilder* sb)  // Current length
```

### `List` - dynamic array

```cpp
#include <List.hh>
```

Implements `IIterable`. A growable array of `Object*` values.

```c
List*    list_new()                                         // Create an empty list
void     list_free(List* list)                              // Free the list and all contained objects
usize    list_get_size(List* list)                          // Number of items
Object*  list_get(List* list, usize index)                  // Get item at index
void     list_set(List* list, usize index, Object* item)    // Set item at index
void     list_add(List* list, Object* item)                 // Append item
void     list_insert(List* list, usize index, Object* item) // Insert item at index
void     list_remove(List* list, usize index)               // Remove item at index
IIterator list_iterator(List* list)                         // Get a forward iterator
```

### `Map` - hash map

```cpp
#include <Map.hh>
```

A hash map from `IKeyable` keys to `Object*` values. Use `String` (or any `IKeyable` type) as the key:

```cpp
String* k = @"hello";
map_set(map, cast<IKeyable>(k), value);
Object* v = map_get(map, cast<IKeyable>(k));
map_remove(map, cast<IKeyable>(k));
string_free(k);
```

```c
Map*     map_new()                                      // Create an empty map
void     map_free(Map* map)                             // Free the map and all stored values
usize    map_get_capacity(Map* map)                     // Current bucket capacity
usize    map_get_filled(Map* map)                       // Number of stored entries
Object*  map_get(Map* map, IKeyable key)                // Lookup by key; returns NULL if absent
void     map_set(Map* map, IKeyable key, Object* value) // Insert or update entry
void     map_remove(Map* map, IKeyable key)             // Remove entry (frees key and value)
```

### `Set` - hash set

```cpp
#include <Set.hh>
```

An unordered set of unique `IKeyable` values.

```c
Set*  set_new()                              // Create an empty set
void  set_free(Set* set)                     // Free the set and all stored keys
usize set_get_size(Set* set)                 // Number of entries
bool  set_contains(Set* set, IKeyable key)   // Membership test
void  set_add(Set* set, IKeyable key)        // Insert (no-op if already present)
void  set_remove(Set* set, IKeyable key)     // Remove entry
```

## Built-in types

`prelude.h` defines short aliases for the standard integer and float types:

```c
i8, i16, i32, i64  // Signed integers
u8, u16, u32, u64  // Unsigned integers
f32, f64           // Floating point
isize              // ptrdiff_t
usize              // size_t
```

## License

Copyright © 2021-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
