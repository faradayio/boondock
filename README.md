# Boondock: Rust library for talking to the Docker daemon

[![Latest version](https://img.shields.io/crates/v/boondock.svg)](https://crates.io/crates/boondock) [![License](https://img.shields.io/crates/l/boondock.svg)](https://opensource.org/licenses/Apache-2.0) [![Build Status](https://travis-ci.org/faradayio/boondock.svg?branch=master)](https://travis-ci.org/faradayio/boondock) [![Build status](https://ci.appveyor.com/api/projects/status/yylowaj7rvdy7b9j?svg=true)](https://ci.appveyor.com/project/emk/boondock) [![Documentation](https://img.shields.io/badge/documentation-docs.rs-yellow.svg)](https://docs.rs/boondock/)

**You may not want this library.** This library is only minimally maintained. It is used by the development tool [cage][], and it does not make much effort to support use-cases beyond that.

It does have a very nice async transport layer based on `hyper`, `hyperlocal`, `rustls` and modern async Rust that you might want to borrow for use in your Docker client. No OpenSSL is involved in any way.

Here are the other Rust Docker clients I know about:

- [rust-docker][] is the original Rust Docker library by Graham Lee, which most of the other libraries are based on (including this one).
- [shiplift][] appears to fairly complete and actively maintained, with lots of downloads. It's still on `hyper` 0.12 at the time of writing.
- [bollard][] is fully async, and at the time of writing, it was based on a modern `hyper` 0.13.

[cage]: http://cage.faraday.io/
[rust-docker]: https://github.com/ghmlee/rust-docker
[shiplift]: https://docs.rs/shiplift/
[bollard]: https://crates.io/crates/bollard

## Examples

For example code, see the [examples directory](./examples).

## Contributing

1. Fork it
2. Create your a new remote upstream repository (`git remote add upstream git@github.com:faradayio/boondock.git`)
3. Commit your changes (`git commit -m 'Add some feature'`)
4. Push to the branch (`git push origin your-branch`)
5. Create new Pull Request
