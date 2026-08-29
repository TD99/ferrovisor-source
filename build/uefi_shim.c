#ifdef _MSC_VER
typedef unsigned __int64 size_t;
#else
#include <stddef.h>
#endif

/* Rust's UEFI target no longer provides this C runtime symbol. */
size_t wcslen(const unsigned short *string) {
    const unsigned short *cursor = string;

    while (*cursor != 0) {
        ++cursor;
    }

    return (size_t)(cursor - string);
}
