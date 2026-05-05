{
  lib,
  rustPlatform,
  pkg-config,
  openssl,
  dbus,

  # for cargo test
  python3,
  gitMinimal,
  cacert,

  rev ? "dirty",
}:
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "deepseek-tui";
  version = "git-${rev}";

  src = ../.;

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  nativeBuildInputs = [
    pkg-config
    dbus
  ];

  buildInputs = [
    openssl
    dbus.lib
  ];

  nativeCheckInputs = [
    python3
    gitMinimal
    cacert
  ];

  cargoBuildFlags = [
    "--package"
    "deepseek-tui-cli"
    "--package"
    "deepseek-tui"
  ];
  cargoTestFlags = finalAttrs.cargoBuildFlags;

  preCheck = ''
    export SSL_CERT_FILE=${cacert}/etc/ssl/certs/ca-bundle.crt
  '';

  meta = {
    description = "Coding agent for DeepSeek models that runs in your terminal";
    homepage = "https://github.com/Hmbown/DeepSeek-TUI";
    license = lib.licenses.mit;
    mainProgram = "deepseek";
  };
})
