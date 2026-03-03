# Verifying releases

All official `vacs` releases are cryptographically signed so you can verify that the binaries and Docker images you download were built by our trusted build pipeline and not modified by a third party.

---

## vacs-client

Each GitHub release for `vacs-client` includes:

- Binary bundles/installers (`.exe`, `.deb`, `.rpm`, `.dmg`, `.app.tar.gz`)
- Per-bundle signatures created by Tauri, used for the automatic updater (`.sig`)
- A checksum file containing the sha256 checksums of all bundles and their signatures (`SHA256SUMS-X.Y.Z.txt`)
- A keyless [cosign](https://github.com/sigstore/cosign) signature (using a GitHub OIDC Token) for the checksum file (`SHA256SUMS-X.Y.Z.txt.sig`, `SHA256SUMS-X.Y.Z.txt.pem`)

Client releases are built, signed and published by the [release-client](../.github/workflows/release-client.yml) GitHub action.

Tauri updater signatures are made using the following public key:

```
dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEJEMTJGNjQxRDY0M0VFRjkKUldUNTdrUFdRZllTdlV5djI5SnlqZUQwZnRDMFNFOWkxSWtpNWcrUTJ2SXlRY2Y4VzF0dWpYdk0K
```

### Verifying client releases

1. Download and install [cosign](https://github.com/sigstore/cosign)
2. Download the desired `vacs-client` release bundle
3. Download the checksum file and its signature and place them in the same directory as the bundle
4. Verify the checksum file signature using cosign

```bash
# Replace X.Y.Z with the actual release version
cosign verify-blob \
  --certificate SHA256SUMS-X.Y.Z.txt.pem \
  --signature SHA256SUMS-X.Y.Z.txt.sig \
  --certificate-identity "https://github.com/vacs-project/vacs/.github/workflows/release-client.yml@refs/tags/vacs-client-vX.Y.Z" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  SHA256SUMS-X.Y.Z.txt
# Check output for "Verified OK"
```

> [!NOTE]  
> Releases before 2.0.0 were published under the `MorpheusXAUT/vacs` repository.
> To verify those older releases, replace `vacs-project/vacs` with `MorpheusXAUT/vacs` in the `--certificate-identity` URL above.

5. Verify checksum of downloaded bundle

```bash
# Replace X.Y.Z with the actual release version
sha256sum --ignore-missing -c SHA256SUMS-X.Y.Z.txt
# Check output for "<bundle file name>: OK"
```

RPM-based distributions are additionally signed using a [GPG key](rpm-signing-key.pem) with the fingerprint `D1325E399B736B07DFDE8AB8E6EB1DC8A5EAB2C5`.  
Import the public key into your RPM package manager to verify the RPM packages:

```bash
rpm --import rpm-signing-key.pem
```

You can then verify the RPM packages using:

```bash
# Replace X.Y.Z with the actual release version
rpm -K vacs-X.Y.Z-1.x86_64.rpm
# Check output for "digests signatures OK"
```

---

## vacs-server

The signaling server is distributed as a Docker image for ease of deployment.

> [!NOTE]  
> Docker images for versions before 2.0.0 were published at `ghcr.io/morpheusxaut/vacs-server`. Starting with 2.0.0, images are published at `ghcr.io/vacs-project/vacs-server`.

Server releases are built, signed and published by the [release-server](../.github/workflows/release-server.yml) GitHub action.

### Verifying server releases

1. Download and install [cosign](https://github.com/sigstore/cosign)
2. Verify the Docker image using cosign

```bash
# Replace X.Y.Z with the actual release version
cosign verify \
  --certificate-identity "https://github.com/vacs-project/vacs/.github/workflows/release-server.yml@refs/tags/vacs-server-vX.Y.Z" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  ghcr.io/vacs-project/vacs-server:X.Y.Z
# Check output for:
# The following checks were performed on each of these signatures:
#  - The cosign claims were validated
#  - Existence of the claims in the transparency log was verified offline
#  - The code-signing certificate was verified using trusted certificate authority certificates
```

> [!NOTE]  
> Releases before 2.0.0 were published under the `MorpheusXAUT/vacs` repository.
> To verify those older releases, replace `vacs-project/vacs` with `MorpheusXAUT/vacs` in the `--certificate-identity` URL and use `ghcr.io/morpheusxaut/vacs-server:X.Y.Z` as the image reference.
