{ ... }: {
  perSystem = { self', pkgs, proto, goPkgs, ... }: {
    packages =
      {
        galoisd = goPkgs.buildGoModule ({
          name = "galoisd";
          src = ./.;
          vendorHash = null;
          doCheck = false;
          meta = {
            mainProgram = "galoisd";
          };
        } // (if pkgs.stdenv.isLinux then {
          nativeBuildInputs = [ pkgs.musl ];
          CGO_ENABLED = 0;
          ldflags = [
            "-linkmode external"
            "-extldflags '-static -L${pkgs.musl}/lib -s -w'"
          ];
        } else { }));

        galoisd-image =
          pkgs.dockerTools.buildImage {
            name = "${self'.packages.galoisd.name}-image";
            copyToRoot = pkgs.buildEnv {
              name = "image-root";
              paths = [ pkgs.coreutils-full pkgs.cacert ];
              pathsToLink = [ "/bin" ];
            };
            config = {
              Entrypoint = [ (pkgs.lib.getExe self'.packages.galoisd) ];
              Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" ];
            };
          };


        generate-prover-proto = pkgs.writeShellApplication {
          name = "generate-prover-proto";
          runtimeInputs =
            [ pkgs.protobuf pkgs.protoc-gen-go pkgs.protoc-gen-go-grpc ];
          text = ''
            find ${proto.galoisd} -type f -regex ".*proto" |\
            while read -r file; do
            echo "Generating $file"
            protoc \
            -I"${proto.cometbls}/proto" \
            -I"${proto.gogoproto}" \
            -I"${proto.galoisd}" \
            --go_out=./grpc --go_opt=paths=source_relative \
            --go-grpc_out=./grpc --go-grpc_opt=paths=source_relative \
            "$file"
            done
          '';
        };

        download-circuit =
          let
            files = pkgs.writeText "files.txt" ''
              /vk.bin
              /pk.bin
              /r1cs.bin
            '';
          in
          pkgs.writeShellApplication {
            name = "download-circuit";
            runtimeInputs = [ pkgs.rclone ];
            text = ''
              if [[ "$#" -ne 1 ]]; then
              echo "Invalid arguments, must be: download-circuit [path]"
              exit 1
              fi
              rclone --progress --no-traverse --http-url "https://circuit.cryptware.io" copy :http:/ "$1" --files-from=${files}
            '';
          };
      };
  };
}
