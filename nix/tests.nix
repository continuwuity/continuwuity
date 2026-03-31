{
  perSystem =
    {
      self',
      lib,
      pkgs,
      ...
    }:
    let
      baseTestScript =
        pkgs.writers.writePython3Bin "do_test" { libraries = [ pkgs.python3Packages.matrix-nio ]; }
          ''
            import asyncio
            import nio


            async def main() -> None:
                # Connect to continuwuity
                client = nio.AsyncClient("http://continuwuity:6167", "alice")

                # Register as user alice
                response = await client.register("alice", "my-secret-password")

                # Log in as user alice
                response = await client.login("my-secret-password")

                # Create a new room
                response = await client.room_create(federate=False)
                print("Matrix room create response:", response)
                assert isinstance(response, nio.RoomCreateResponse)
                room_id = response.room_id

                # Join the room
                response = await client.join(room_id)
                print("Matrix join response:", response)
                assert isinstance(response, nio.JoinResponse)

                # Send a message to the room
                response = await client.room_send(
                    room_id=room_id,
                    message_type="m.room.message",
                    content={
                        "msgtype": "m.text",
                        "body": "Hello continuwuity!"
                    }
                )
                print("Matrix room send response:", response)
                assert isinstance(response, nio.RoomSendResponse)

                # Sync responses
                response = await client.sync(timeout=30000)
                print("Matrix sync response:", response)
                assert isinstance(response, nio.SyncResponse)

                # Check the message was received by continuwuity
                last_message = response.rooms.join[room_id].timeline.events[-1].body
                assert last_message == "Hello continuwuity!"

                # Leave the room
                response = await client.room_leave(room_id)
                print("Matrix room leave response:", response)
                assert isinstance(response, nio.RoomLeaveResponse)

                # Close the client
                await client.close()


            if __name__ == "__main__":
                asyncio.run(main())
          '';
    in
    {
      # run some nixos tests as checks
      checks = lib.pipe self'.packages [
        # we take all packages (names)
        builtins.attrNames
        # we filter out all packages that end with `-bin` (which we are interested in for testing)
        (builtins.filter (lib.hasSuffix "-bin"))
        # for each of these binaries we built the basic nixos test
        #
        # this test was initially yoinked from
        #
        # https://github.com/NixOS/nixpkgs/blob/960ce26339661b1b69c6f12b9063ca51b688615f/nixos/tests/matrix/continuwuity.nix
        (builtins.concatMap (
          name:
          builtins.map
            (
              { config, suffix }:
              {
                name = "test-${name}-${suffix}";
                value = pkgs.testers.runNixOSTest {
                  inherit name;

                  nodes = {
                    continuwuity = {
                      services.matrix-continuwuity = {
                        enable = true;
                        package = self'.packages.${name};
                        settings = config;
                        extraEnvironment.RUST_BACKTRACE = "yes";
                      };
                      networking.firewall.allowedTCPPorts = [ 6167 ];
                    };
                    client.environment.systemPackages = [ baseTestScript ];
                  };

                  testScript = ''
                    start_all()

                    with subtest("start continuwuity"):
                          continuwuity.wait_for_unit("continuwuity.service")
                          continuwuity.wait_for_open_port(6167)

                    with subtest("ensure messages can be exchanged"):
                          client.succeed("${lib.getExe baseTestScript} >&2")
                  '';

                };
              }
            )
            [
              {
                suffix = "base";
                config = {
                  global = {
                    server_name = name;
                    address = [ "0.0.0.0" ];
                    allow_registration = true;
                    yes_i_am_very_very_sure_i_want_an_open_registration_server_prone_to_abuse = true;
                  };
                };
              }
              {
                suffix = "with-room-version";
                config = {
                  global = {
                    server_name = name;
                    address = [ "0.0.0.0" ];
                    allow_registration = true;
                    yes_i_am_very_very_sure_i_want_an_open_registration_server_prone_to_abuse = true;
                    default_room_version = "12";
                  };
                };
              }
            ]
        ))
        builtins.listToAttrs
      ];
    };
}
