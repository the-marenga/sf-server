# Usage with official SF client
The official, unmodified *Shakes & Fidget* client requires you to use SSL. To be more specific, all the URLs are hardcoded with a `https://` prefix, and it's a lot easier to set up a reverse proxy like nginx, since you can then simply use the *Shakes & Fidget* client as-is from the official [*Shakes & Fidget* website](https://sfgame.net). The only thing then left to do is to override the `https://sfgame.net/config.json` response in your browser, such that it returns the Nginx address (https://localhost:6768 in this example) as one of the servers.

## Setting up Nginx as a reverse proxy
### Docker
Take note of the `conf/nginx.conf` file. Out of the box it assumes, that the `sf-server` runs on port 6767 (http protocol with no TLS/SSL), and will then publish port 6768 (https protocol). If you rename your certificates, you will also change them here accordingly. The ports also need to reflect the ports you have configured in the `docker-compose.yml` file and the `sf-server` itself.

After configuring the ports to your liking, proceed to generating a certificate.

#### Generating a self-signed certificate
To get started quickly, you can use self-signed certificated, for your convenience you can use the information from the `example_cert_data_localhost.cnf` file by first running the following command:
```bash
openssl req -x509 -nodes -days 365 -newkey rsa:2048 -keyout localhost.key -out localhost.pem -config example_cert_data_localhost.cnf -sha256
```
With the key, you may then proceed to generate the certificate:
```bash
openssl req -new -x509 -nodes -sha256 -days 365 -key localhost.key -out localhost.crt
```
Move the generated files to the `certs` directory.

#### Starting the docker stack
To start the docker stack, run the following command in the directory of the `docker-compose.yml` file:
```bash
docker-compose up -d
```

#### stopping the docker stack
```bash
docker-compose down
```

### Non-docker setups
Nah man... Use docker.
