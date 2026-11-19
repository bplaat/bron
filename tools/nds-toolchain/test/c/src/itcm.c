// ARM code in ITCM that calls back into Thumb code in main RAM

int thumb_add(int a, int b);

int itcm_call(int a, int b) {
    return thumb_add(a, b) * 2;
}
