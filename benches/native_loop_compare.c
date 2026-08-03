#include <errno.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#if defined(__clang__) || defined(__GNUC__)
#define NOINLINE __attribute__((noinline))
#else
#define NOINLINE
#endif

#if defined(RPHP_FORCE_SCALAR_LOOP) && (defined(__clang__) || defined(__GNUC__))
#define LOOP_BARRIER(value) __asm__ __volatile__("" : "+r"(value))
#else
#define LOOP_BARRIER(value) ((void)0)
#endif

static double monotonic_seconds(void) {
    struct timespec timestamp;
    if (clock_gettime(CLOCK_MONOTONIC, &timestamp) != 0) {
        perror("clock_gettime");
        exit(2);
    }
    return (double)timestamp.tv_sec + (double)timestamp.tv_nsec / 1000000000.0;
}

static int64_t parse_iterations(const char *value) {
    char *end = NULL;
    errno = 0;
    long long parsed = strtoll(value, &end, 10);
    if (errno != 0 || end == value || *end != '\0' || parsed < 0) {
        fprintf(stderr, "invalid iteration count: %s\n", value);
        exit(2);
    }
    return (int64_t)parsed;
}

static NOINLINE int64_t run_sum_loop(int64_t iterations) {
    int64_t sum = 0;
    for (int64_t induction = 0; induction < iterations; induction++) {
        sum += induction;
        LOOP_BARRIER(sum);
    }
    return sum;
}

static NOINLINE int64_t run_modulo_branch_loop(int64_t iterations) {
    int64_t sum = 0;
    for (int64_t induction = 0; induction < iterations; induction++) {
        if ((induction % 2) == 0) {
            sum += induction;
        }
        LOOP_BARRIER(sum);
    }
    return sum;
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: %s sum|modulo ITERATIONS\n", argv[0]);
        return 2;
    }

    int64_t iterations = parse_iterations(argv[2]);
    int64_t result;
    double start = monotonic_seconds();
    if (strcmp(argv[1], "sum") == 0) {
        result = run_sum_loop(iterations);
    } else if (strcmp(argv[1], "modulo") == 0) {
        result = run_modulo_branch_loop(iterations);
    } else {
        fprintf(stderr, "unknown benchmark: %s\n", argv[1]);
        return 2;
    }
    double elapsed = monotonic_seconds() - start;

    printf("%" PRId64 "|%.9f\n", result, elapsed);
    return 0;
}
