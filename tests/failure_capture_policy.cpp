// SPDX-License-Identifier: GPL-3.0-only
#include "../runtime/orchestrator/src/failure_capture_policy.h"
#include <cstdio>
int main() {
    rsf::diagnostics::FailureCapturePolicy policy;
    int failures=0;
    const auto check=[&](bool okay) { if (!okay) ++failures; };
    check(!policy.claim(false,false,-10));
    check(!policy.claim(true,true,-10));
    check(!policy.claim(true,false,0));
    check(policy.captured==0);
    check(policy.claim(true,false,-10)); // failed temporal evaluation
    check(!policy.claim(true,true,-3));  // active capture must not be replaced
    check(policy.claim(true,false,-3)); // spatial recovery without evaluation
    check(policy.claim(true,false,-1));
    check(policy.captured==3);
    check(!policy.claim(true,false,-10));
    check(!policy.claim(false,false,-10));
    check(!policy.claim(true,false,-10)); // toggling cannot defeat session cap
    rsf::diagnostics::SequenceCapturePolicy sequence;
    check(!sequence.claim(1,1,true,false));
    sequence.request();
    check(!sequence.claim(1,1,false,false));
    check(!sequence.claim(1,1,true,true));
    check(!sequence.claim(0,1,true,false));
    check(sequence.remaining==4);
    check(sequence.claim(1,1,true,false));
    check(!sequence.claim(1,1,true,false));
    check(sequence.claim(1,2,true,false));
    check(sequence.claim(1,3,true,false));
    check(sequence.claim(1,4,true,false));
    check(!sequence.claim(1,5,true,false));
    sequence.request();
    check(sequence.claim(2,1,true,false));
    std::printf("failure capture policy: %d failures\n",failures);
    return failures ? 1 : 0;
}
