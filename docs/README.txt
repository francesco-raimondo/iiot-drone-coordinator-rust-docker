

Make sure you have already installed all the necessary requirements for rust, you can follow the official guide here https://rust-lang.org/tools/install/


Modalità di sviluppo del progetto:
Ho scaricato WSL
Ho utilizzato AntigravityIDE scaricando le estensioni di Rust dal negozio delle estensioni di visual studio code (INCOLLA QUI LINK)

Avendo tutti i file in WSL ho cambiato il workspace di AntigravityIDE dal suo workspace di default, ovvero quello di windows11, al workspace di WSLin questo modo:
- Schiaccia Ctrl + Shift + P
- cerca e seleziona "Remote-WSL: Connect to WSL"
- Antigravity will immediately setup the new WSL-connected workspace. 
- A questo punto schiacciando su "open folder" puoi navigare nel file system di WSL, selezionare il tuo folder di interesse e aprirlo
- Ora dovrai riscaricare le estensioni anche in questo workspace, andando sul negozio delle estensioni e scaricando quelle di tuo interesse

Per realizzare questo progetto è stato utilizzato docker, Avendo uan configurazione Windows11 - WSL, assicurati di avere installato docker desktop su windows11, a questo punto:

- vai nelle impostazioni di docker desktop (sezione 'General') e attiva l'integrazione con la tua distro WSL2 mettendo la spunta su "Use the WSL 2 Based engine".
- vai nelle impostazioni di docker desktop (sezione "Resources) e metti la spunta su "Enable integration with my default WSL distro"
- nella stessa schermata attiva la distro di Ubuntu spostando l'interruttore su ON
- Applica le modifiche e riavvia docker desktop

Per verificare che tutto è stato eseguito correttamente:
- entra nel workspace di wsl da terminale avviando il terminale di windows e scrivendo "wsl"
- runna "docker version"
- L'output atteso dovrebbe essere qualcosa di questo tipo:

your_username@AsusLP023W:~/Desktop$ docker version
Client:
 Version:           29.2.1
 API version:       1.53
 Go version:        go1.25.6
 Git commit:        a5c7197
 Built:             Mon Feb  2 17:16:41 2026
 OS/Arch:           linux/amd64
 Context:           default

Server: Docker Desktop 4.61.0 (219004)
 Engine:
  Version:          29.2.1
  API version:      1.53 (minimum version 1.44)




MAKEFILE

1) run <COMANDO> per effettuare il pull delle immagini docker necessarie per il funzionamento del progetto. Le immagini sono:
ros-jazzy-base ( per runnare i container associati ai droni)
ros-jazzy-desktop-full ( per runnare il container in cui si svolge la simulazione, la versione desktop-full include anche gli strumenti grafici come Gazebo)




BUILD CUSTOM IMAGE OF ROS 
docker run -it --rm \
  --env="DISPLAY=$DISPLAY" \
  --env="WAYLAND_DISPLAY=$WAYLAND_DISPLAY" \
  --env="XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR" \
  --volume="/tmp/.X11-unix:/tmp/.X11-unix:rw" \
  --volume="/mnt/wslg:/mnt/wslg:rw" \
  --device /dev/dxg \
  --gpus all \
my-ros2-humble-desktop-full:latest



docker run -it --rm \
  --env="DISPLAY=$DISPLAY" \
  --env="WAYLAND_DISPLAY=$WAYLAND_DISPLAY" \
  --env="XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR" \
  --volume="/tmp/.X11-unix:/tmp/.X11-unix:rw" \
  --volume="/mnt/wslg:/mnt/wslg:rw" \
  --device /dev/dxg \
  --gpus all \
my-ros2-jazzy-desktop-full:latest





FROM osrf/ros:jazzy-desktop-full

# Installiamo Gazebo Sim (Harmonic) e il bridge per ROS2 Jazzy
RUN apt-get update && apt-get install -y \
    ros-jazzy-ros-gz \
    && rm -rf /var/lib/apt/lists/*

# In Jazzy, il comando ufficiale è 'gz sim'
CMD ["gz", "sim", "-v", "4", "-r", "empty.sdf"]



docker build -f Dockerfile_gazebo -t my-ros2-jazzy-desktop-full .


docker run -it --rm \
  --env="DISPLAY=$DISPLAY" \
  --env="WAYLAND_DISPLAY=$WAYLAND_DISPLAY" \
  --env="XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR" \
  --volume="/tmp/.X11-unix:/tmp/.X11-unix:rw" \
  --volume="/mnt/wslg:/mnt/wslg:rw" \
  --device /dev/dxg \
  --gpus all \
my-ros2-jazzy-desktop-full:latest


ho verificato che docker usi la GPU e la sua, ne sono sicuro. 

ora, ho notato che, provando a runnare l'immagine "my-ros-jazzy-desktop-full" da sola con questo comando:

docker run -it --rm \
  --env="DISPLAY=$DISPLAY" \
  --env="WAYLAND_DISPLAY=$WAYLAND_DISPLAY" \
  --env="XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR" \
  --volume="/tmp/.X11-unix:/tmp/.X11-unix:rw" \
  --volume="/mnt/wslg:/mnt/wslg:rw" \
  --device /dev/dxg \
  --gpus all \
my-ros2-jazzy-desktop-full:latest


funziona tutto, la schermata compare. ma quando la runno con docker compose up, sapendo che il docker compose file per ora è questo:

[docker-compose.yml](file;vscode-remote://wsl%2Bubuntu/home/francesco/Desktop/IOT/Project/docker-compose.yml)  non funziona 
