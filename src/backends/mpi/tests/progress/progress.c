/* Credit-return experiment for a 1-slot-per-credit ring, Open MPI, two ranks on one node.
 *
 * Rank 0 (sender) owns the acknowledgement counter. Rank 1 (receiver) owns the slots.
 * The sender fills the ring (cap frames), then waits for a credit with NO MPI calls
 * (pure local loads), up to STALL_MS. After a stall it makes exactly one MPI call
 * (MPI_Iprobe), so a progress-dependent credit can still complete and the run ends.
 *
 * Receiver consumes frame k (local polls only), then acknowledges with one of:
 *   rma      MPI_Fetch_and_op(+1) on the sender's counter + MPI_Win_flush   (the only MPI call)
 *   acc      MPI_Accumulate(+1), no flush                                   (never completed locally)
 *   shm      atomic store into the sender's counter via MPI_Win_shared_query pointer
 *
 * Logs (CLOCK_MONOTONIC ns, same machine so comparable across ranks):
 *   sender:   Full events (t0 before read, sent, acked read), credit-seen events, stalls
 *   receiver: ack-return events (t after the acknowledgement call returned)
 * MODE is rma|acc|aop|shm. usage: mpirun -n 2 progress MODE CAP K STALL_MS OUTDIR
 */
#include <mpi.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static uint64_t now_ns(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

enum mode { M_RMA, M_ACC, M_SHM, M_AOP };


int main(int argc, char **argv) {
  if (argc != 6) {
    fprintf(stderr, "usage: %s rma|acc|shm CAP K STALL_MS OUTDIR\n", argv[0]);
    return 2;
  }
  enum mode mode = !strcmp(argv[1], "rma") ? M_RMA : !strcmp(argv[1], "acc") ? M_ACC
                  : !strcmp(argv[1], "aop") ? M_AOP : M_SHM;
  const int cap = atoi(argv[2]);
  const long K = atol(argv[3]);
  const uint64_t stall_ns = (uint64_t)atol(argv[4]) * 1000000ull;
  const char *outdir = argv[5];
  MPI_Datatype pair;

  int provided;
  MPI_Init_thread(&argc, &argv, MPI_THREAD_MULTIPLE, &provided);
  MPI_Type_contiguous(2, MPI_UINT64_T, &pair);
  MPI_Type_commit(&pair);
  int rank, size;
  MPI_Comm_rank(MPI_COMM_WORLD, &rank);
  MPI_Comm_size(MPI_COMM_WORLD, &size);
  if (size != 2) { fprintf(stderr, "needs exactly 2 ranks\n"); MPI_Abort(MPI_COMM_WORLD, 2); }

  /* Slots live on the receiver (rank 1): cap x {seq, payload}, 16 bytes each. */
  /* Counter lives on the sender (rank 0): one uint64. */
  MPI_Win win_slots, win_ack;
  uint64_t *slots = NULL, *ack = NULL;
  uint64_t *peer_ack = NULL;   /* shm mode: receiver's pointer into sender's counter */
  uint64_t *peer_slots = NULL; /* shm mode: sender's pointer into receiver's slots */
  MPI_Aint sz;
  int disp;
  if (mode == M_SHM) {
    MPI_Win_allocate_shared(rank == 1 ? (MPI_Aint)(cap * 16) : 0, 1, MPI_INFO_NULL,
                            MPI_COMM_WORLD, &slots, &win_slots);
    MPI_Win_allocate_shared(rank == 0 ? 16 : 0, 1, MPI_INFO_NULL, MPI_COMM_WORLD, &ack, &win_ack);
    MPI_Win_shared_query(win_slots, 1, &sz, &disp, &peer_slots);
    MPI_Win_shared_query(win_ack, 0, &sz, &disp, &peer_ack);
  } else {
    MPI_Win_allocate(rank == 1 ? (MPI_Aint)(cap * 16) : 0, 1, MPI_INFO_NULL, MPI_COMM_WORLD,
                     &slots, &win_slots);
    MPI_Win_allocate(rank == 0 ? 16 : 0, 1, MPI_INFO_NULL, MPI_COMM_WORLD, &ack, &win_ack);
  }
  if (rank == 0) memset(ack, 0, 16);
  if (rank == 1) memset(slots, 0, cap * 16);
  MPI_Win_lock_all(MPI_MODE_NOCHECK, win_slots);
  MPI_Win_lock_all(MPI_MODE_NOCHECK, win_ack);
  MPI_Barrier(MPI_COMM_WORLD);

  char path[512];
  snprintf(path, sizeof path, "%s/%s-cap%d.%s", outdir, argv[1], cap, rank == 0 ? "sender" : "receiver");
  FILE *log = fopen(path, "w");
  if (!log) { perror(path); MPI_Abort(MPI_COMM_WORLD, 3); }

  if (rank == 0) {
    /* Sender. Counter is read with plain loads: no MPI call while waiting. */
    uint64_t sent = 0;
    for (long k = 1; k <= K; k++) {
      uint64_t t0 = now_ns();
      uint64_t acked = __atomic_load_n(&ack[0], __ATOMIC_ACQUIRE);
      if (sent - acked >= (uint64_t)cap) {
        fprintf(log, "F %ld %llu %llu %llu\n", k, (unsigned long long)t0, (unsigned long long)sent,
                (unsigned long long)acked);
        uint64_t start = now_ns();
        int stalled = 0;
        for (;;) {
          acked = __atomic_load_n(&ack[0], __ATOMIC_ACQUIRE);
          if (sent - acked < (uint64_t)cap) break;
          if (!stalled && now_ns() - start > stall_ns) {
            int flag;
            MPI_Iprobe(MPI_ANY_SOURCE, MPI_ANY_TAG, MPI_COMM_WORLD, &flag, MPI_STATUS_IGNORE);
            fprintf(log, "P %ld %llu\n", k, (unsigned long long)now_ns());
            stalled = 1;
          }
        }
        fprintf(log, "S %ld %llu %llu %d\n", k, (unsigned long long)now_ns(),
                (unsigned long long)acked, stalled);
      }
      /* Send frame k into position (k-1)%cap. */
      uint64_t pos = (uint64_t)((k - 1) % cap);
      uint64_t payload[2] = {(uint64_t)k, (uint64_t)k};
      if (mode == M_SHM) {
        peer_slots[pos * 2 + 1] = payload[1];
        __atomic_store_n(&peer_slots[pos * 2], payload[0], __ATOMIC_RELEASE);
      } else {
        MPI_Put(payload, 16, MPI_BYTE, 1, (MPI_Aint)(pos * 16), 16, MPI_BYTE, win_slots);
        MPI_Win_flush(1, win_slots);
      }
      sent++;
    }
  } else {
    /* Receiver. Polls its own slots with plain loads; the ack is its only MPI call. */
    const uint64_t one = 1;
    for (long k = 1; k <= K; k++) {
      uint64_t pos = (uint64_t)((k - 1) % cap);
      uint64_t seq;
      while ((seq = __atomic_load_n(&slots[pos * 2], __ATOMIC_ACQUIRE)) != (uint64_t)k) {
      }
      if (slots[pos * 2 + 1] != (uint64_t)k) {
        fprintf(log, "CORRUPT %ld\n", k);
      }
      /* Consume: the slot is free again once acknowledged. */
      if (mode == M_RMA) {
        uint64_t old;
        MPI_Fetch_and_op(&one, &old, MPI_UINT64_T, 0, 0, MPI_SUM, win_ack);
        MPI_Win_flush(0, win_ack);
      } else if (mode == M_AOP) {
        /* Non-intrinsic accumulate, then a flush that completes only when the target applies it. */
        /* MPI_REPLACE with a derived type: not an intrinsic atomic, so the target applies it. */
        uint64_t cumulative[2] = {(uint64_t)k, (uint64_t)k};
        MPI_Accumulate(cumulative, 1, pair, 0, 0, 1, pair, MPI_REPLACE, win_ack);
        MPI_Win_flush(0, win_ack);
      } else if (mode == M_ACC) {
        MPI_Accumulate(&one, 1, MPI_UINT64_T, 0, 0, 1, MPI_UINT64_T, MPI_SUM, win_ack);
      } else {
        __atomic_fetch_add(peer_ack, 1, __ATOMIC_RELEASE);
      }
      fprintf(log, "R %ld %llu\n", k, (unsigned long long)now_ns());
    }
  }
  fclose(log);
  MPI_Barrier(MPI_COMM_WORLD); /* the only collective after the run */
  MPI_Win_unlock_all(win_slots);
  MPI_Win_unlock_all(win_ack);
  MPI_Win_free(&win_slots);
  MPI_Win_free(&win_ack);
  MPI_Type_free(&pair);
  MPI_Finalize();
  return 0;
}
