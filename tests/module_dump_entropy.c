/* Entropy measurement against inputs whose answers are known independently. */

#include <rescaleframe/module_dump.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int passed = 1;

static void check(int condition, const char* message)
{
    if (!condition) {
        fprintf(stderr, "%s\n", message);
        passed = 0;
    }
}

static void check_near(double actual, double expected, double tolerance, const char* message)
{
    const double difference = actual > expected ? actual - expected : expected - actual;
    if (difference > tolerance) {
        fprintf(stderr, "%s (got %.4f, expected %.4f)\n", message, actual, expected);
        passed = 0;
    }
}

int main(void)
{
    enum { size = 65536 };
    unsigned char* buffer = (unsigned char*)malloc(size);
    if (!buffer) {
        return 2;
    }

    memset(buffer, 0, size);
    check_near(rsf_shannon_entropy(buffer, size), 0.0, 1e-9,
               "A single repeated byte must measure zero entropy.");

    /* Every byte value exactly as often as every other is the maximum, 8 bits per byte. */
    for (int index = 0; index < size; ++index) {
        buffer[index] = (unsigned char)(index & 0xff);
    }
    check_near(rsf_shannon_entropy(buffer, size), 8.0, 1e-9,
               "A uniform byte distribution must measure the full eight bits.");

    /* Two values in equal proportion carry one bit. */
    for (int index = 0; index < size; ++index) {
        buffer[index] = (unsigned char)(index & 1 ? 0xff : 0x00);
    }
    check_near(rsf_shannon_entropy(buffer, size), 1.0, 1e-9,
               "Two equally likely values must measure one bit.");

    check_near(rsf_shannon_entropy(buffer, 0), 0.0, 1e-9, "An empty range must measure zero.");
    check_near(rsf_shannon_entropy(NULL, size), 0.0, 1e-9, "A null range must measure zero.");

    /* Sampling must agree with a whole-range measurement on uniform data, and must fall back to
       one measurement when the range is smaller than a window. */
    for (int index = 0; index < size; ++index) {
        buffer[index] = (unsigned char)(index & 0xff);
    }
    check_near(rsf_sampled_entropy(buffer, size, 4096, 8), 8.0, 1e-9,
               "Sampling uniform data must still measure the full eight bits.");
    check_near(rsf_sampled_entropy(buffer, 256, 4096, 8), rsf_shannon_entropy(buffer, 256), 1e-9,
               "A range smaller than one window must be measured whole.");
    check_near(rsf_sampled_entropy(buffer, size, 0, 8), 0.0, 1e-9,
               "A zero window must not be measured.");

    /* Half ciphertext, half zeroes must land between the two, which is what makes a partly
       decrypted section visible rather than passing as decrypted. */
    memset(buffer, 0, size / 2);
    const double mixed = rsf_sampled_entropy(buffer, size, 4096, 16);
    check(mixed > 0.5 && mixed < 7.5, "A partly uniform range must measure between the extremes.");

    free(buffer);
    return passed ? 0 : 1;
}
