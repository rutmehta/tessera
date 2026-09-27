#include <libraw/libraw.h>
#include <stdio.h>
int main(int argc, char **argv) {
 if(argc != 3) return 2;
 libraw_data_t *r=libraw_init(0);
 int e=libraw_open_file(r,argv[1]);
 if(!e) e=libraw_unpack(r);
 if(!e) e=libraw_dcraw_process(r);
 if(e) {fprintf(stderr,"%s\n",libraw_strerror(e));return 1;}
 r->params.output_tiff=0;
 e=libraw_dcraw_ppm_tiff_writer(r,argv[2]);
 libraw_close(r);
 return e ? 1 : 0;
}
