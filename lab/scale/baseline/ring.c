/* Plain-MPI baseline for scale_ring: W ranks in a ring. Each step a rank posts MPI_Irecv from both
 * neighbours, MPI_Isend to both, then MPI_Waitall. Payload as in the trame program: bytes 0..8 the step,
 * bytes 8..16 the sender. Each frame's step is checked against the step expected from that neighbour.
 * WARM steps run first and are not timed; the clock runs for STEPS steps. Rank 0 gathers the spans and
 * prints one JSON line per rank (bench "mpi_ring"; no cap field, since MPI has no route capacity).
 * Build: mpicc -O2 -o ring ring.c
 * Run:   mpirun -n W ./ring W STEPS SIZE   (SIZE in bytes, at least 16) */
#include <mpi.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define WARM 10

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
    int rank, nranks, i;
    MPI_Init(&argc, &argv);
    MPI_Comm_rank(MPI_COMM_WORLD, &rank);
    MPI_Comm_size(MPI_COMM_WORLD, &nranks);
    if (argc != 4) {
        if (rank == 0) fprintf(stderr, "usage: %s W STEPS SIZE\n", argv[0]);
        MPI_Abort(MPI_COMM_WORLD, 2);
    }
    int w = atoi(argv[1]);
    long steps = atol(argv[2]);
    int sz = atoi(argv[3]);
    if (nranks != w) {
        if (rank == 0) fprintf(stderr, "W=%d but mpirun started %d ranks\n", w, nranks);
        MPI_Abort(MPI_COMM_WORLD, 2);
    }
    if (w < 3) fail(rank, "W must be at least 3 (as scale_ring)");
    if (steps < 1) fail(rank, "STEPS must be at least 1");
    if (sz < 16) fail(rank, "SIZE must be at least 16");

    int left = (rank + nranks - 1) % nranks, right = (rank + 1) % nranks;
    unsigned char *sbuf = calloc(1, (size_t)sz);
    unsigned char *lbuf = calloc(1, (size_t)sz);
    unsigned char *rbuf = calloc(1, (size_t)sz);
    long total = WARM + steps, s;
    unsigned long long t0 = 0;
    MPI_Request req[4];
    for (s = 0; s < total; s++) {
        if (s == WARM) {
            MPI_Barrier(MPI_COMM_WORLD); /* not timed: starts the measured steps together */
            t0 = now_ns();
        }
        put64(sbuf, (unsigned long long)s);
        put64(sbuf + 8, (unsigned long long)rank);
        MPI_Irecv(lbuf, sz, MPI_BYTE, left, 0, MPI_COMM_WORLD, &req[0]);
        MPI_Irecv(rbuf, sz, MPI_BYTE, right, 0, MPI_COMM_WORLD, &req[1]);
        MPI_Isend(sbuf, sz, MPI_BYTE, left, 0, MPI_COMM_WORLD, &req[2]);
        MPI_Isend(sbuf, sz, MPI_BYTE, right, 0, MPI_COMM_WORLD, &req[3]);
        MPI_Waitall(4, req, MPI_STATUSES_IGNORE);
        if (get64(lbuf) != (unsigned long long)s || get64(rbuf) != (unsigned long long)s)
            fail(rank, "a frame out of order");
    }
    unsigned long long ns = now_ns() - t0;
    unsigned long long *all = rank == 0 ? malloc((size_t)nranks * sizeof *all) : NULL;
    MPI_Gather(&ns, 1, MPI_UNSIGNED_LONG_LONG, all, 1, MPI_UNSIGNED_LONG_LONG, 0, MPI_COMM_WORLD);
    if (rank == 0) {
        for (i = 0; i < nranks; i++)
            printf("{\"bench\":\"mpi_ring\",\"workers\":%d,\"worker\":%d,\"steps\":%ld,\"size\":%d,\"ns\":%llu}\n",
                   w, i, steps, sz, all[i]);
        free(all);
    }
    free(sbuf);
    free(lbuf);
    free(rbuf);
    MPI_Finalize();
    return 0;
}
