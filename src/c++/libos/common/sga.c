// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

#include <dmtr/sga.h>

#include <dmtr/annot.h>
#include <dmtr/fail.h>
#include <dmtr/types.h>
#include <errno.h>
#include <stdio.h>

int dmtr_sgalen(size_t *len_out, const dmtr_sgarray_t *sga) {
    DMTR_NOTNULL(EINVAL, len_out);
    *len_out = 0;
    DMTR_NOTNULL(EINVAL, sga);

    size_t len = 0;
    for (size_t i = 0; i < sga->sga_numsegs; ++i) {
        len += sga->sga_segs[i].sgaseg_len;
    }

    *len_out = len;
    return 0;
}

int dmtr_sgafree(dmtr_sgarray_t *sga) {
    // we haven't got a good solution for communicating how to free
    // scatter/gather arrays.

    if (NULL == sga) {
        return 0;
    }

    if (NULL == sga->sga_buf) {
        for (size_t i = 0; i < sga->sga_numsegs; ++i) {
            //printf("freeing a scatter-gather array: %lx\n",sga->sga_segs[i].sgaseg_buf);
            free(sga->sga_segs[i].sgaseg_buf);
        }
    } else {
        free(sga->sga_buf);
        //   printf("freeing a scatter-gather array: %lx\n", sga->sga_buf);
    }

    return 0;
}

// free a particular segment within an SGA
int dmtr_sgafree_seg(dmtr_sgarray_t *sga, int seg) {
    if (NULL == sga) {
        return 0;
    }

    if (NULL == sga->sga_buf) {
        free(sga->sga_segs[seg].sgaseg_buf);
    }

    return 0;
}
