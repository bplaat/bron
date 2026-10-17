<a href="https://youtu.be/XQE8vEB6AW0"><img align="right" src="docs/bob-the-builder.jpg" width="200" alt="Bob the Builder"></a>

# Bassie's Obvious Builder (bob)

A simple build system for my projects, because I really like the simplicity of Cargo and I detest all the other build systems :^)

## Supported project types

- Clang/GCC languages (C, C++, Objective-C, Objective-C++, Assembly)
- JVM languages (Java, Kotlin)
- Android (Java, Kotlin, XML)

### Supported package types

- Java Archive (.jar)
- macOS bundle (.app)
- Android APK (.apk)

### Supported unit test libraries

- CUnit (C, C++, Objective-C, Objective-C++)
- JUnit (Java, Kotlin)

## Getting Started

### Creating a C project

- Install bob

    ```sh
    cargo install --git https://github.com/bplaat/bron bob
    ```

- Create a `bob.toml` manifest file in your project root:

    ```toml
    [project]
    name = "hello"
    version = "0.1.0"
    ```

- Create a `src/main.c` source file:

    ```c
    #include <stdio.h>
    int main(void) {
        printf("Hello bob!\n");
        return 0;
    }
    ```

- Then run `bob` to build and run the project:

    ```sh
    bob run
    ```

### Running C unit tests

- You can create unit tests inline in your C code, for example if you have this function:

    ```c
    int add(int a, int b) {
        return a + b;
    }
    ```

- You can create a unit test for it like this:

    ```c
    #ifdef TEST
    #include <CUnit/Basic.h>
    void test_add(void) {
        CU_ASSERT_EQUAL(add(3, 4), 7);
        CU_ASSERT_EQUAL(add(-5, 6), 1);
    }
    #endif
    ```

- Then run `bob` to build and run the tests:

    ```sh
    bob test
    ```

- This will also print the CUnit test report

### JVM and Android builds

Java projects need a JDK, and Kotlin projects also need `kotlinc`. Android builds need
JDK 17 (or JDK 21 as a fallback) and `ANDROID_HOME` pointing to an SDK installation
with platform 37, build-tools 37, command-line tools, and platform-tools.

Bob downloads and extracts dependency JARs and adds DEX files to APKs in Rust;
`wget`, `zip`, and `unzip` are not required. Maven Central downloads try
`repo.maven.apache.org` first, then `repo1.maven.org`, with up to three attempts per
host, backoff, connection/read/write timeouts, and bounded redirects.

Downloaded JARs are checked for ZIP integrity, including entry CRCs, before being
stored atomically in the cache. Downloads use a lock per artifact, reuse valid
cached JARs, and replace corrupt ones. Cache filenames include a hash of the source
URL or local path to keep different artifacts separate.

## License

Copyright © 2025-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../LICENSE) license.
