# Rust Audio Chat
## Objectif 
Projet de cours 5TC INSA Lyon
Nous voulons développer un chat audio en temps réel entre plusieurs clients sur Rust.

## Architecture
Un serveur relais centralisé et hébergé sur AWS et plusieurs clients.
L'architecture est détaillée ci dessous :

<img width="1795" height="851" alt="Diagramme drawio" src="https://github.com/user-attachments/assets/f4a99f87-0a11-4959-ac29-96f822688b71" />

## Fonctionnalités client
Notre système se connecte automatiquement aux périphériques audios entrée/sortie par défaut de la machine client, à travers le serveur audio Pipewire et son API JACK.
On ouvre aussi un serveur web pour l'interface côté client.
Le client choisit son pseudo au démarrage et se connecte au serveur relais pour échanger les paquets via QUIC.

## Fonctionnalités serveur
Le serveur relais reçoit les paquets audio des clients et les diffuse à tous les clients connectés.
Les paquets sont ensuite triés côté client pour ne pas rejouer son propre audio
Diffuse également la liste des clients connectés.

## Détail des paquets
Il y a trois types de paquets échangés, identifiés par un ID au début du paquet :
- 0x01 : paquet audio, contient l'ID du client
- 0x02 : paquet d'identification pour transmettre le pseudo client au serveur
- 0x03 : paquet de broadcast de la liste des clients connectés 

## Installation
Voici les paquets nécessaire côté client :
```
sudo apt-get update
sudo apt install jack-tools libjack-jackd2-dev pipewire-jack pipewire-pulse pulseaudio-utils build-essential cmake
```
Les crates utilisées sont : 
#### Côté client:
jack = "0.13.4"
opus = "0.3.1"
ringbuf = "0.4.8"
tokio = { version = "1", features = ["full"] }
quinn = "0.11"
rustls = { version = "0.23", default-features = false, features = ["ring"] }
rand = "0.8"
byteorder = "1.5"
axum = "0.7"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tower-http = { version = "0.5", features = ["fs"] }

#### Côté serveur:
tokio = { version = "1", features = ["full"] }
quinn = "0.11"
rustls = { version = "0.23", default-features = false, features = ["ring"] }
rcgen = "0.13"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
byteorder = "1.5"

## Références 
https://github.com/quinn-rs/quinn/tree/main/quinn/examples
https://docs.rs/ringbuf/latest/ringbuf/
https://docs.rs/opus/latest/opus/

