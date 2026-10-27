/* Plain-MPI baseline for scale_barrier: W ranks, one warm-up MPI_Barrier, then ROUNDS timed MPI_Barrier
 * calls per rank. Each rank times its own loop on CLOCK_MONOTONIC; rank 0 gathers the spans and prints one
 * JSON line per rank, in the same shape as the trame program's lines (bench "mpi_barrier").
 * Build: mpicc -O2 -o barrier barrier.c
 * Run:   mpirun -n W ./barrier W ROUNDS */
#include <mpi.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

static unsigned long long now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (unsigned long long)ts.tv_sec * 1000000000ULL + (unsigned long long)ts.tv_nsec;
}

int main(int argc, char **argv) {
    int rank, size, i;
    MPI_Init(&argc, &argv);
    MPI_Comm_rank(MPI_COMM_WORLD, &rank);
    MPI_Comm_size(MPI_COMM_WORLD, &size);
    if (argc != 3) {
        if (rank == 0) fprintf(stderr, "usage: %s W ROUNDS\n", argv[0]);
        MPI_Abort(MPI_COMM_WORLD, 2);
    }
    int w = atoi(argv[1]);
    long rounds = atol(argv[2]);
    if (size != w) {
        if (rank == 0) fprintf(stderr, "W=%d but mpirun started %d ranks\n", w, size);
        MPI_Abort(MPI_COMM_WORLD, 2);
    }
    MPI_Barrier(MPI_COMM_WORLD);
    unsigned long long t0 = now_ns();
    for (i = 0; i < rounds; i++) MPI_Barrier(MPI_COMM_WORLD);
    unsigned long long ns = now_ns() - t0;

    unsigned long long *all = rank == 0 ? malloc((size_t)size * sizeof *all) : NULL;
    MPI_Gather(&ns, 1, MPI_UNSIGNED_LONG_LONG, all, 1, MPI_UNSIGNED_LONG_LONG, 0, MPI_COMM_WORLD);
    if (rank == 0) {
        for (i = 0; i < size; i++)
            printf("{\"bench\":\"mpi_barrier\",\"workers\":%d,\"worker\":%d,\"rounds\":%ld,\"ns\":%llu}\n",
                   w, i, rounds, all[i]);
        free(all);
    }
    MPI_Finalize();
    return 0;
}
