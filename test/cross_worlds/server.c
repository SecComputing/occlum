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
#define BUFF_LEN 32

int main() {
    int sock, client;
    struct sockaddr_un server = {};
    char buf[BUFF_LEN] = {};

    sock = socket(AF_UNIX, SOCK_STREAM, 0);
    if (sock < 0) {
        THROW_ERROR("failed to create socket");
    }

    server.sun_family = AF_UNIX;
    strcpy(server.sun_path, PATH);
    if (bind(sock, (struct sockaddr *)&server, sizeof(struct sockaddr_un)) < 0) {
        close(sock);
        THROW_ERROR("failed to bind");
    }

    if (listen(sock, 1) < 0) {
        close(sock);
        THROW_ERROR("failed to listen");
    }

    client = accept(sock, NULL, 0);
    if (client < 0) {
        close(sock);
        THROW_ERROR("failed to accept");
    }

    if (read(client, buf, BUFF_LEN) <= 0) {
        close_files(2, client, sock);
        THROW_ERROR("failed to read msg");
    }

    if (strncmp(buf, MSG, strlen(MSG)) != 0) {
        close_files(2, client, sock);
        THROW_ERROR("msg mismatches");
    }

    close_files(2, client, sock);
    // TODO: unlink the file in host when running in libos
    unlink(PATH);
    return 0;
}
