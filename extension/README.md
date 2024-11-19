# Info

This Firefox extensions adds a custom domain to the server selection of the
official S&F client.

# Usage

To use this you need to have setup a https enabled server.
You can have a look at the reverse_proxy if you want to do this locally, use
a service like cloudlflare tunnel to point a url to your local machine, or
actually run this on an actual server behind a https certified proxy.

In any case you need to have the game accept you domain as a valid server. To
do so, change the `SERVER_URL` constant in the `background.js` file to your url.
The client has some hardcoded expectations for the url format, so you would
want to follow the "server_name.domain.tld" format.

Special server_names like `dev` and `beta` can influence the behaviour of the
client. Just make sure to NOT use `beta`. Otherwise names like `w1`, `f1`, `s1`
may be localized to `World 1`/`Welt 1`/.. in the client, which may make it
difficult to distinguish them from the official severs.

You can further change the category under which the server appears. By default
this defaults to `fu` for fused servers, since that allows custom messages.

To actually run this extension, go to the url `about:debugging#/runtime/this-firefox`,
click on load temparary Add-on and select the background.js file from this repo.

You can then visit [https://sfgame.net/config.json](https://sfgame.net/config.json)
to make sure there is a new entry at the bottom of the list with your server url.

The official client will now show you server url as an official server, when
you login, or register a new account. Once you do that, you can even
remove this extension, since the client saves the url.
