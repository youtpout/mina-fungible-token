/*
 * The C interface of `shared/native/src/ffi.rs`, as Swift sees it.
 *
 * Every call takes a JSON request and returns a JSON response, exactly like
 * the JNI entry points the Android app uses. The returned pointer is owned by
 * Rust: hand it back to `mina_string_free` or it leaks.
 */

#ifndef MINA_H
#define MINA_H

/* The backend versions and the git revisions the library was built from. */
char *mina_backend_info(void);

/* Proves a `FungibleToken.transfer` and submits it. Timings included. */
char *mina_transfer(const char *request);

/* Reads an address's token balance from the configured node. */
char *mina_token_balance(const char *request);

/* Releases a string returned by any of the functions above. */
void mina_string_free(char *value);

#endif /* MINA_H */
