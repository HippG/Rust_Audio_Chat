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
Le client choisit son pseudo au démarrage et se connecte au serveur relais pour lui envoyer les données audio via QUIC.

## Fonctionnalités serveur
Le serveur relais reçoit les données audio des clients et les diffuse à tous les clients connectés.
Les paquets sont ensuite triés côté client pour ne pas rejouer son propre audio

## Détails des paquets
Il y a trois types de paquets échangés, identifiés par un ID au début du paquet :
- 0x01 : paquet audio, contient l'ID du client
- 0x02 : paquet d'identification pour transmettre le pseudo client au serveur
- 0x03 : paquet de broadcast de la liste des clients connectés 



## Références 

