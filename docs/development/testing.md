# Testing

## Complement

Have a look at [Complement's repository][complement] for an explanation of what
it is.

To test against Complement, with Nix (or [Lix](https://lix.systems) and
[direnv installed and set up][direnv] (run `direnv allow` after setting up the hook), you can:

* Run `./bin/complement "$COMPLEMENT_SRC"` to build a Complement image, run
the tests, and output the logs and results to the specified paths. This will also output the OCI image
at `result`
* Run `nix build .#complement` from the root of the repository to just build a
Complement OCI image outputted to `result` (it's a `.tar.gz` file)
* Or download the latest Complement OCI image from the CI workflow artifacts
output from the commit/revision you want to test (e.g. from main)
[here][ci-workflows]

If you want to use your own prebuilt OCI image (such as from our CI) without needing
Nix installed, put the image at `complement_oci_image.tar.gz` in the root of the repo
and run the script.

If you're on macOS and need to build an image, run `nix build .#linux-complement`.

We have a Complement fork as some tests have needed to be fixed. This can be found
at: <https://forgejo.ellis.link/continuwuation/complement>

[ci-workflows]:
https://forgejo.ellis.link/continuwuation/continuwuity/actions/?workflow=ci.yml&actor=0&status=1
[complement]: https://github.com/matrix-org/complement
[direnv]: https://direnv.net/docs/hook.html
