# NixOS module for the Snap 24 MCP server, served over streamable HTTP behind
# nginx with an ACME certificate.
#
#   inputs.snap24.url = "git+ssh://git@github.com/marcustut/snap24";
#   # ...
#   services.snap24-mcp = {
#     enable = true;
#     domain = "snap24.marcustut.me";
#   };
{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.snap24-mcp;
in
{
  options.services.snap24-mcp = {
    enable = lib.mkEnableOption "the Snap 24 MCP server";

    domain = lib.mkOption {
      type = lib.types.str;
      description = "Public hostname. Served at https://<domain>/mcp.";
      example = "snap24.marcustut.me";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8788;
      description = "Loopback port the server listens on.";
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.snap24-mcp;
      defaultText = lib.literalExpression "snap24.packages.\${system}.snap24-mcp";
      description = "The snap24-mcp package to run.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.snap24-mcp = {
      description = "Snap 24 MCP server";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];

      serviceConfig = {
        ExecStart = "${cfg.package}/bin/snap24-mcp --http 127.0.0.1:${toString cfg.port}";
        # rmcp only accepts loopback `Host` headers unless told otherwise.
        Environment = "SNAP24_MCP_ALLOWED_HOSTS=${cfg.domain}";
        Restart = "always";
        RestartSec = 3;

        DynamicUser = true;
        NoNewPrivileges = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectControlGroups = true;
        RestrictNamespaces = true;
        LockPersonality = true;
      };
    };

    services.nginx.virtualHosts.${cfg.domain} = {
      forceSSL = true;
      enableACME = true;

      locations."/mcp" = {
        proxyPass = "http://127.0.0.1:${toString cfg.port}";
        extraConfig = ''
          # MCP answers over SSE, so the response must stream through.
          proxy_buffering off;
          proxy_cache off;
          proxy_http_version 1.1;
          proxy_set_header Connection "";
          proxy_read_timeout 3600s;
          proxy_send_timeout 3600s;
        '';
      };
    };
  };
}
