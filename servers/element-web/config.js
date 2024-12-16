import fs from 'node:fs';
// Read base config
let baseConfig = JSON.parse(fs.readFileSync('./config.json', 'utf8'));
// Server list
const servers = JSON.parse(fs.readFileSync('./servers.json', 'utf8'));
// Default server config
const defaultServerConfig = "element.ellis.link";
const defaultConfigPath = './public/config.json';

// raim.ist config
const raimConfig = await fetch('https://riot.raim.ist/config.json').then(res => res.json());

baseConfig = {
    ...baseConfig,
    enable_presence_by_hs_url: {
        ...raimConfig.enable_presence_by_hs_url,
        ...baseConfig.enable_presence_by_hs_url,
    },
    setting_defaults: {
        ...baseConfig.setting_defaults,
        custom_themes: raimConfig.setting_defaults.custom_themes,
    },

};
// biome-ignore lint/complexity/noForEach: <explanation>
Object.keys(servers).forEach(server => {
    // Get server config
    const serverConfig = servers[server];
    // Create overridden config
    const overriddenConfig = {
        ...baseConfig,
        default_server_name: serverConfig.serverName,
        default_server_config: {
            ...baseConfig?.default_server_config,
            "m.homeserver": {
                ...baseConfig.default_server_config["m.homeserver"],
                base_url: serverConfig.baseUrl
            },
            "org.matrix.msc3575.proxy": {
                ...baseConfig.default_server_config["org.matrix.msc3575.proxy"],
                url: serverConfig.baseUrl
            }
        },
        room_directory: {
            ...baseConfig.room_directory,
            servers: [
                serverConfig.serverName,
                ...baseConfig.room_directory.servers.filter(s => s !== serverConfig.serverName)
            ]
        }
    };
    // Write config
    const configPath = `./public/config.${server}.json`;
    console.log(`Writing config to ${configPath}`);
    fs.writeFileSync(configPath, JSON.stringify(overriddenConfig, null, 4));
    // write default config
    if (server === defaultServerConfig) {
        console.log(`Writing default config to ${defaultConfigPath}`);
        fs.writeFileSync(defaultConfigPath, JSON.stringify(overriddenConfig, null, 4));
    }
});
