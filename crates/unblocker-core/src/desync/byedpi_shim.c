/**
 * byedpi_shim.c — Thin library wrapper around byedpi (ciadpi) for embedding
 * into libunblocker_core.so on Android.
 *
 * Exposes a clean start/run/stop/cleanup API so Rust can manage the DPI
 * desync engine without spawning a subprocess.
 *
 * ByeDPI uses global state (params, server_fd, signal handlers), so only
 * one instance may run per process — same constraint as the subprocess model.
 */

#include <string.h>
#include <unistd.h>
#include <sys/socket.h>

/* Byedpi internals — use the real headers so struct layout is correct. */
#include "params.h"
#include "proxy.h"

/* Functions defined in main.c (linked into the static lib). */
extern int parse_args(int argc, char **argv);
extern void clear_params(char *line, char **argv);

/* Server fd for shutdown. Also used by byedpi's internal signal handlers. */
static int byedpi_server_fd = -1;

/**
 * Parse CLI arguments and create the listening socket.
 *
 * @param argc  Argument count (same as main()).
 * @param argv  Argument vector (same as main()).
 * @return Listening socket fd (>= 0) on success, -1 on error.
 */
int byedpi_start(int argc, char **argv) {
    int res = parse_args(argc, argv);
    if (res < 0) {
        return -1;
    }

    int fd = listen_socket(&params.laddr);
    if (fd < 0) {
        return -1;
    }

    byedpi_server_fd = fd;
    return fd;
}

/**
 * Run the byedpi event loop. Blocks until shutdown.
 *
 * @param fd  Listening socket fd from byedpi_start().
 * @return 0 on clean exit, -1 on error.
 */
int byedpi_run(int fd) {
    return start_event_loop(fd);
}

/**
 * Signal the event loop to stop (causes byedpi_run to return).
 * Thread-safe: may be called from any thread.
 */
void byedpi_stop(void) {
    int fd = byedpi_server_fd;
    if (fd >= 0) {
        shutdown(fd, SHUT_RDWR);
    }
}

/**
 * Free all byedpi-allocated memory (params, mempool, dp linked list).
 * Must be called after byedpi_run returns.
 */
void byedpi_cleanup(void) {
    byedpi_server_fd = -1;
    clear_params((char *)0, (char **)0);
}
