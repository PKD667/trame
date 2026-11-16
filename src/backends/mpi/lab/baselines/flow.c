/* Plain-MPI baseline for bench_flow: N = H * PER ranks, PER per host, each sending a stream of SIZE-byte
 * frames to one other rank and receiving one stream. PATTERN ring: rank r sends to r + 1 (mod N); shift:
 * to r + PER (mod N), every frame between hosts when H >= 2. Frames go in windows of CAP: post CAP
 * MPI_Irecv and CAP MPI_Isend, then MPI_Waitall, as osu_bw does; CAP plays the part of the route's
 * capacity. Bytes 0..8 of a frame are its number, 8..16 its sender; each is checked. WARM frames run
 * first and are not timed. Rank 0 gathers the spans and prints one JSON line per rank.
 * Build: mpicc -O2 -o mpi_flow flow.c
 * Run:   mpirun -n N ./mpi_flow PATTERN PER SIZE CAP STEPS */
#include <mpi.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define WARM 64

static unsigned long long now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (unsigned long long)ts.tv_sec * 1000000000ULL + (unsigned long long)ts.tv_nsec;
}

static void put64(unsigned char *p, unsigned long long v) { memcpy(p, &v, 8); }
static unsigned long long get64(const unsigned char *p) { unsigned long long v; memcpy(&v, p, 8); return v; }

static void fail(int rank, const char *msg) {
    fprintf(stderr, "rank %d: %s\n", rank, msg);
    MPI_Abort(MPI_COMM_WORLD, 1);
}

int main(int argc, char **argv) {
    int rank, n, i;
    MPI_Init(&argc, &argv);
    MPI_Comm_rank(MPI_COMM_WORLD, &rank);
    MPI_Comm_size(MPI_COMM_WORLD, &n);
    if (argc != 6) {
        if (rank == 0) fprintf(stderr, "usage: %s ring|shift PER SIZE CAP STEPS\n", argv[0]);
        MPI_Abort(MPI_COMM_WORLD, 2);
    }
    int shift = strcmp(argv[1], "shift") == 0;
    int per = atoi(argv[2]), sz = atoi(argv[3]), cap = atoi(argv[4]);
    long steps = atol(argv[5]);
    if (!shift && strcmp(argv[1], "ring") != 0) fail(rank, "PATTERN is ring or shift");
    if (per < 1 || n % per != 0 || n < 2) fail(rank, "the ranks are H * PER, at least 2");
    if (sz < 16 || cap < 1 || steps < 1) fail(rank, "SIZE >= 16, CAP >= 1, STEPS >= 1");
    if (shift && n == per) shift = 0; /* one host: shift is ring, as in bench_flow */
    int step = shift ? per : 1;
    int to = (rank + step) % n, from = (rank + n - step) % n;
    unsigned char *sbuf = calloc((size_t)cap, (size_t)sz), *rbuf = calloc((size_t)cap, (size_t)sz);
    MPI_Request *req = malloc(2 * (size_t)cap * sizeof *req);
    long total = WARM + steps, sent = 0, got = 0;
    unsigned long long t0 = 0;
    while (sent < total) {
        if (sent >= WARM && t0 == 0) t0 = now_ns();
        long b = total - sent < cap ? total - sent : cap;
        for (i = 0; i < b; i++) {
            MPI_Irecv(rbuf + (size_t)i * sz, sz, MPI_BYTE, from, 0, MPI_COMM_WORLD, &req[i]);
            put64(sbuf + (size_t)i * sz, (unsigned long long)(sent + i));
            put64(sbuf + (size_t)i * sz + 8, (unsigned long long)rank);
            MPI_Isend(sbuf + (size_t)i * sz, sz, MPI_BYTE, to, 0, MPI_COMM_WORLD, &req[b + i]);
        }
        MPI_Waitall(2 * (int)b, req, MPI_STATUSES_IGNORE);
        for (i = 0; i < b; i++, got++)
            if (get64(rbuf + (size_t)i * sz) != (unsigned long long)got || get64(rbuf + (size_t)i * sz + 8) != (unsigned long long)from)
                fail(rank, "a frame out of order or misrouted");
        sent += b;
    }
    unsigned long long ns = now_ns() - t0;
    unsigned long long *all = rank == 0 ? malloc((size_t)n * sizeof *all) : NULL;
    MPI_Gather(&ns, 1, MPI_UNSIGNED_LONG_LONG, all, 1, MPI_UNSIGNED_LONG_LONG, 0, MPI_COMM_WORLD);
    if (rank == 0) {
        for (i = 0; i < n; i++)
            printf("{\"bench\":\"flow\",\"pattern\":\"%s\",\"hosts\":%d,\"host\":%d,\"per\":%d,\"g\":%d,\"size\":%d,\"cap\":%d,\"steps\":%ld,\"ns\":%llu}\n",
                   argv[1], n / per, i / per, per, i, sz, cap, steps, all[i]);
        free(all);
    }
    free(sbuf);
    free(rbuf);
    free(req);
    MPI_Finalize();
    return 0;
}
