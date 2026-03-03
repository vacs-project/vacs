# Verifying releases

All official `vacs` releases are cryptographically signed so you can verify that the binaries and Docker images you download were built by our trusted build pipeline and not modified by a third party.

---

## vacs-client

Each GitHub release for `vacs-client` includes:

- Binary bundles/installers (`.exe`, `.deb`, `.rpm`, `.dmg`, `.app.tar.gz`)
- Per-bundle signatures created by Tauri, used for the automatic updater (`.sig`)
- A checksum file containing the sha256 checksums of all bundles and their signatures (`SHA256SUMS-X.Y.Z.txt`)
- A keyless [cosign](https://github.com/sigstore/cosign) bundle (using a GitHub OIDC Token) for the checksum file (`SHA256SUMS-X.Y.Z.txt.bundle.json`)

Client releases are built, signed and published by the [release-client](../.github/workflows/release-client.yml) GitHub action.

Tauri updater signatures are made using the following public key:

```
dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEJEMTJGNjQxRDY0M0VFRjkKUldUNTdrUFdRZllTdlV5djI5SnlqZUQwZnRDMFNFOWkxSWtpNWcrUTJ2SXlRY2Y4VzF0dWpYdk0K
```

### Verifying client releases

1. Download and install [cosign](https://github.com/sigstore/cosign)
2. Download the desired `vacs-client` release bundle
3. Download the checksum file and its cosign bundle and place them in the same directory as the bundle
4. Verify the checksum file using cosign

```bash
# Replace X.Y.Z with the actual release version
cosign verify-blob \
  --bundle SHA256SUMS-X.Y.Z.txt.bundle.json \
  --certificate-identity "https://github.com/vacs-project/vacs/.github/workflows/release-client.yml@refs/tags/vacs-client-vX.Y.Z" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  SHA256SUMS-X.Y.Z.txt
# Check output for "Verified OK"
```

> [!NOTE]  
> **Older releases** used a different cosign signature format. If the release
> contains `SHA256SUMS-X.Y.Z.txt.sig` and `SHA256SUMS-X.Y.Z.txt.pem` instead
> of a `.bundle.json` file, verify with:
>
> ```bash
> cosign verify-blob \
>   --signature SHA256SUMS-X.Y.Z.txt.sig \
>   --certificate SHA256SUMS-X.Y.Z.txt.pem \
>   --certificate-identity "https://github.com/vacs-project/vacs/.github/workflows/release-client.yml@refs/tags/vacs-client-vX.Y.Z" \
>   --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
>   SHA256SUMS-X.Y.Z.txt
> ```
>
> Releases before 2.0.0 were published under the `MorpheusXAUT/vacs` repository.
> To verify those, additionally replace `vacs-project/vacs` with `MorpheusXAUT/vacs`
> in the `--certificate-identity` URL.

5. Verify checksum of downloaded bundle

```bash
# Replace X.Y.Z with the actual release version
sha256sum --ignore-missing -c SHA256SUMS-X.Y.Z.txt
# Check output for "<bundle file name>: OK"
```

### Verifying build attestations

Starting with versions _after_ 2.0.0, each release binary also carries a GitHub build attestation (SLSA provenance) and an SBOM attestation. You can verify them with the [GitHub CLI](https://cli.github.com/):

```bash
# Verify build provenance of a specific artifact
gh attestation verify <artifact-file> --repo vacs-project/vacs

# Verify the SBOM attestation
gh attestation verify <artifact-file> --repo vacs-project/vacs \
  --predicate-type https://spdx.dev/Document
```

### RPM package signing

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
> To verify those older releases, replace `vacs-project/vacs` with `MorpheusXAUT/vacs`
> in the `--certificate-identity` URL and use `ghcr.io/morpheusxaut/vacs-server:X.Y.Z`
> as the image reference.

### Verifying build attestations

Starting with version _after_ 2.0.0, the Docker image also carries a GitHub build attestation (SLSA provenance) that can be verified with the [GitHub CLI](https://cli.github.com/):

```bash
gh attestation verify \
  oci://ghcr.io/vacs-project/vacs-server:X.Y.Z \
  --repo vacs-project/vacs
```

Additionally, the Docker image includes an embedded SBOM and [SLSA provenance](https://slsa.dev/spec/v1.0/provenance) generated by BuildKit (stored as OCI annotations on the image manifest).

---

## vacs-data (Tools CLI)

Tool binaries are published from the [vacs-data](https://github.com/vacs-project/vacs-data) repository. Each release includes:

- Pre-built binaries for Linux, macOS, and Windows
- A checksum file (`SHA256SUMS-X.Y.Z.txt`)
- A [cosign](https://github.com/sigstore/cosign) bundle for the checksum file (`SHA256SUMS-X.Y.Z.txt.bundle.json`)
- GitHub [build attestations](https://docs.github.com/en/actions/security-guides/using-artifact-attestations-to-establish-provenance-for-builds) (SLSA provenance + SBOM) for each binary

### Verifying tools releases

Checksum and cosign verification works the same way as for `vacs-client`:

```bash
# Verify the checksum file signature
cosign verify-blob \
  --bundle SHA256SUMS-X.Y.Z.txt.bundle.json \
  --certificate-identity "https://github.com/vacs-project/vacs-data/.github/workflows/release-tools.yml@refs/tags/vacs-data-vX.Y.Z" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  SHA256SUMS-X.Y.Z.txt

# Verify checksum of downloaded binary
sha256sum --ignore-missing -c SHA256SUMS-X.Y.Z.txt
```

### Verifying build attestations

```bash
# Verify build provenance
gh attestation verify <binary-file> --repo vacs-project/vacs-data

# Verify the SBOM attestation
gh attestation verify <binary-file> --repo vacs-project/vacs-data \
  --predicate-type https://spdx.dev/Document
```
