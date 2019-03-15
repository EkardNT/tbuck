# This script takes care of testing your crate

set -ex

main() {
    cross build --target $TARGET
    cross build --target $TARGET --release

    if [ ! -z $DISABLE_TESTS ]; then
        return
    fi

    cross test --target $TARGET
    cross test --target $TARGET --release

    # per https://github.com/rust-embedded/cross/commit/902b2e8300c8665f277e8f5874e495ed7d4341d3
    # `cross` will automatically install clippy.
    cross clippy --target $TARGET --all-targets
}

# we don't run the "test phase" when doing deploys
if [ -z $TRAVIS_TAG ]; then
    main
fi
