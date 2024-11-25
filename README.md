# sf-server — A Shakes & Fidget Server Implementation in Rust
## About
This project is an open-source implementation of the *Shakes & Fidget* server. It is based upon various community efforts of reverse engineering the protocol and the game mechanics. You can use this project to host your own (private) server, we will however not provide you with the client or any game assets, but as the browser-based client is publicly available on the [*Shakes & Fidget* website](https://sfgame.net), you can use that to connect to your server. **Important:** Keep in mind, that if you decide to let other people play on your server, we strongly advise against you distributing the client either, the legalities of this are questionable at best.

Currently, this project is in the early stages of development, and it is unclear, if it will ever be fully complete. The goal is of course to be 100% compatible with official game clients, and to offer all the features the official servers do. However, due to the nature of an actively updated game like *Shakes & Fidget*, this is a moving target. If you feel you can contribute to this project in any way, feel free to do so. Any help is appreciated, be it in the form of code, documentation, or even just testing and bug reporting.


## Getting Started
To run this project for yourself, you can simply clone this repository and build the server yourself, or check the [releases](https://github.com/the-marenga/sf-server/releases) page for pre-built binaries.

### Connecting clients to your server
Launching the server by itself will publish the server on `http://0.0.0.0:6768` by default. Only `HTTP` (without) TLS/SSL is supported by the Rust application itself, that becomes a challenge when you want to use the official *Shakes & Fidget* client, to connect. That is because it prefixes all server connections with *https://*. To overcome this, it is recommended to use a reverse proxy **(II)** in order to have the traffic between the client and the server run over the `HTTPS` protocol.

In addition, a *Firefox* extension is provided as a quality of life feature, if your browser does not persist the `config.json` overrides you set. Instructions for manually overriding the `config.json` response can also be found in the respective section below.

#### (I) Usage with Firefox extension
In the [/extension](https://github.com/the-marenga/sf-server/tree/main/extension) directory of this repository, you can find a *Firefox* add-on, that can be used, if your browser forces you to override the `config.json` request over and over again by not persisting it after e.g. a browser restart. Some *Chromium*-based browsers seem to persist the overrides, others don't. Best you see for yourself how your Browser behaves and install the extension as needed.

Detailed instructions on how to install the extension can be found in the [README.md](https://github.com/the-marenga/sf-server/tree/main/extension/README.md) file in the same directory.

#### (II) Usage with *nginx* as a reverse proxy
The more permanent solution is to use a reverse proxy like *nginx*. This setup has only be done once on the server that hosts the  *sf-server*, as opposed to the *Firefox* extension, which needs to be installed on every client/player that wants to connect to the server.

Detailed instructions on how to set up *nginx* as a reverse proxy using [*Docker*](https://www.docker.com/) can be found in the [reverse_proxy](https://github.com/the-marenga/sf-server/tree/main/reverse_proxy) directory of this repository.

Advanced users could also make a *Docker* image of this project, to also run in the same *Docker* stack as *nginx*. Currently, we do not provide the image or Dockerfile outselves, but if you make one your contribution is welcome.

#### Overriding the `config.json` response manually
To override the `config.json` response in your browser, you will have to use the developer tools of your browser, depending on what browser you use, the steps might vary slightly. When in doubt, you can always use a search engine to find out how to do it in your specific browser. In *Chromium*-based browsers, this only has to be done once, if you find yourself having to repeat this, consider using the **(I)** *Firefox* extension.

The following steps are for *Chrome*:
1. Open the developer tools by pressing `F12` or `Ctrl+Shift+I`.
2. Go to the `Network` tab.
3. Reload the page by pressing `F5`.
4. Find the `config.json` request in the list of requests.
5. Right-click on the request and click "Override content".

You will be presented with a `JSON` formatted list of the official *Shakes & Fidget* servers. You can now add your server to the list, by adding an object to the `servers` array. The a single object from the list should look something like this:
```json
{
    "i": 458,
    "d": "s1.sfgame.eu",
    "c": "eu"
}
```
You can now either edit an existing entry, or add a new one. You may even delete them all, and make the array have just one entry, which is your server. Whichever way you choose, assuming you are running the server on `localhost`, the object should look similar to this (the port could be different of course, depending on your setup):
```json
{
    "i": 458,
    "d": "localhost:6768",
    "c": "eu"
}
```
Hit `Ctrl+S` to save the changes, and reload the page. You should now be able to create a character on your server and play the game 🥳
