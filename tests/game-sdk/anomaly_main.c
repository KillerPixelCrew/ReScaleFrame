// SPDX-License-Identifier: MIT
extern int rsf_test_anomaly_plugin(void);
extern int rsf_test_anomaly_lifecycle(void);

int main(void)
{
#ifdef RSF_ANOMALY_LIFECYCLE_FIXTURE
    return rsf_test_anomaly_lifecycle();
#else
    return rsf_test_anomaly_plugin();
#endif
}
