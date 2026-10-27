/* Plain-MPI baseline for scale_graph: W ranks on the circulant graph of scale/graph_net.rs, with the same
 * splitmix64, offsets and xorshift64 recurrence, so each rank's checksum equals the trame worker's. Each
 * step a rank computes ITERS rounds, posts MPI_Irecv from each in-neighbour and MPI_Isend to each
 * out-neighbour, then MPI_Waitall. The first WARM steps are a warm-up; the clock runs for STEPS steps.
 * Rank 0 gathers the spans and prints one JSON line per rank (bench "mpi_graph", "worker" = rank).
 * Build: mpicc -O2 -o mpi_graph scale/baseline/graph.c
 * Run:   mpirun -n W ./mpi_graph W STEPS SIZE DEGREE ITERS SEED   (SIZE 64 or 1024) */
#include <mpi.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define WARM 10
typedef unsigned long long u64;

static u64 splitmix_draw(u64 *s) {
    u64 z;
    *s += 0x9E3779B97F4A7C15ULL;
    z = *s;
    z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ULL;
    z = (z ^ (z >> 27)) * 0x94D049BB133111EBULL;
    return z ^ (z >> 31);
}

static u64 rounds(u64 x, u64 n) {
    u64 i;
    for (i = 0; i < n; i++) {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
    }
    return x;
}

static void put64(unsigned char *p, u64 v) { memcpy(p, &v, 8); }
static u64 get64(const unsigned char *p) { u64 v; memcpy(&v, p, 8); return v; }

static void fail(int rank, const char *msg) {
    fprintf(stderr, "rank %d: %s\n", rank, msg);
    MPI_Abort(MPI_COMM_WORLD, 1);
}

int main(int argc, char **argv) {
    int rank, nranks, i, k;
    MPI_Init(&argc, &argv);
    MPI_Comm_rank(MPI_COMM_WORLD, &rank);
    MPI_Comm_size(MPI_COMM_WORLD, &nranks);
    if (argc != 7) {
        if (rank == 0) fprintf(stderr, "usage: %s W STEPS SIZE DEGREE ITERS SEED\n", argv[0]);
        MPI_Abort(MPI_COMM_WORLD, 2);
    }
    u64 w = strtoull(argv[1], NULL, 10), steps = strtoull(argv[2], NULL, 10);
    u64 sz = strtoull(argv[3], NULL, 10), degree = strtoull(argv[4], NULL, 10);
    u64 iters = strtoull(argv[5], NULL, 10), seed = strtoull(argv[6], NULL, 10);
    if ((u64)nranks != w) {
        if (rank == 0) fprintf(stderr, "W=%llu but mpirun started %d ranks\n", w, nranks);
        MPI_Abort(MPI_COMM_WORLD, 2);
    }
    if (w < 2 || degree == 0 || degree > w - 1 || (sz != 64 && sz != 1024) || steps == 0) {
        if (rank == 0) fprintf(stderr, "usage: W>=2 STEPS>=1 SIZE in {64,1024} DEGREE in 1..W-1 ITERS SEED\n");
        MPI_Abort(MPI_COMM_WORLD, 2);
    }

    /* The same offsets as graph_net.rs: draw until DEGREE distinct values in 1..W-1. */
    u64 *offs = malloc(degree * sizeof *offs);
    u64 st = seed, nd = 0;
    while (nd < degree) {
        u64 o = 1 + splitmix_draw(&st) % (w - 1), j, dup = 0;
        for (j = 0; j < nd; j++)
            if (offs[j] == o) dup = 1;
        if (!dup) offs[nd++] = o;
    }

    int *outs = malloc(degree * sizeof *outs), *ins = malloc(degree * sizeof *ins);
    for (k = 0; k < (int)degree; k++) {
        outs[k] = (int)((rank + offs[k]) % w);
        ins[k] = (int)((rank + w - offs[k]) % w);
    }
    unsigned char *sbuf = calloc(1, sz), *rbuf = calloc(degree, sz);
    MPI_Request *req = malloc(2 * degree * sizeof *req);
    u64 x = (u64)rank + 1, total = WARM + steps, s;
    double t0 = 0, compute = 0;
    for (s = 0; s < total; s++) {
        double c0;
        if (s == WARM) t0 = MPI_Wtime();
        c0 = MPI_Wtime();
        x = rounds(x, iters);
        if (s >= WARM) compute += MPI_Wtime() - c0;
        put64(sbuf, s);
        put64(sbuf + 8, (u64)rank);
        for (k = 0; k < (int)degree; k++)
            MPI_Irecv(rbuf + (size_t)k * sz, (int)sz, MPI_BYTE, ins[k], 0, MPI_COMM_WORLD, &req[k]);
        for (k = 0; k < (int)degree; k++)
            MPI_Isend(sbuf, (int)sz, MPI_BYTE, outs[k], 0, MPI_COMM_WORLD, &req[degree + k]);
        MPI_Waitall(2 * (int)degree, req, MPI_STATUSES_IGNORE);
        for (k = 0; k < (int)degree; k++)
            if (get64(rbuf + (size_t)k * sz) != s) fail(rank, "a frame out of order");
    }
    double span = MPI_Wtime() - t0;
    /* Checksum as scale/graph.rs: the state XOR the sum over every frame received, ((sender+1) << 32 | (step+1)). */
    u64 acc = 0;
    for (k = 0; k < (int)degree; k++)
        for (s = 0; s < total; s++)
            acc += (((u64)ins[k] + 1) << 32) | (s + 1);
    u64 local[3] = {(u64)(span * 1e9), (u64)(compute * 1e9), x ^ acc};
    u64 *all = rank == 0 ? malloc((size_t)nranks * 3 * sizeof *all) : NULL;
    MPI_Gather(local, 3, MPI_UNSIGNED_LONG_LONG, all, 3, MPI_UNSIGNED_LONG_LONG, 0, MPI_COMM_WORLD);
    if (rank == 0) {
        for (i = 0; i < nranks; i++)
            printf("{\"bench\":\"mpi_graph\",\"workers\":%d,\"worker\":%d,\"steps\":%llu,\"size\":%llu,\"degree\":%llu,\"iters\":%llu,\"ns\":%llu,\"compute_ns\":%llu,\"checksum\":%llu}\n",
                   nranks, i, steps, sz, degree, iters, all[3 * i], all[3 * i + 1], all[3 * i + 2]);
        free(all);
    }
    free(offs); free(outs); free(ins); free(sbuf); free(rbuf); free(req);
    MPI_Finalize();
    return 0;
}
