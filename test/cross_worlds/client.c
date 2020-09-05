#include <sys/socket.h>
#include <sys/types.h>
#include <sys/un.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "test.h"

#define MSG "From client"
#define PATH "cross_worlds_unix_socket"

int main() {
    int sock;
    struct sockaddr_un server = {};

    sock = socket(AF_UNIX, SOCK_STREAM, 0);
    if (sock < 0) {
        THROW_ERROR("failed to create socket");
    }

    server.sun_family = AF_UNIX;
    strcpy(server.sun_path, PATH);

    if (connect(sock, (struct sockaddr *)&server, sizeof(struct sockaddr_un)) < 0) {
        close(sock);
        THROW_ERROR("failed to connect");
    }

    if (write(sock, MSG, sizeof(MSG)) < 0) {
        close(sock);
        THROW_ERROR("failed to send data");
    }

    close(sock);
    return 0;
}
