// SPDX-License-Identifier: MIT
#ifndef RSF_VK_NGX_ENTRIES_H
#define RSF_VK_NGX_ENTRIES_H

/* Private loader ABI. Missing SDK symbols remain null. */
struct rsf_vk_ngx_entries {
    void *init_project_id;
    void *shutdown1;
    void *get_capability_params;
    void *allocate_params;
    void *destroy_params;
    void *feature_requirements;
    void *instance_extensions;
    void *device_extensions;
    void *create_feature;
    void *create_feature1;
    void *evaluate_feature;
    void *release_feature;
};

const struct rsf_vk_ngx_entries *rsf_vk_ngx_get_entries(void);
const char *rsf_vk_ngx_load_error(void);

#endif
