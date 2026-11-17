// confid.exe <installation id> - prints the confirmation ID that activates Windows XP offline
#include "confid.h"

int main(int argc, char **argv)
{
    if (argc != 2) return 2;
    char result[49] = {};
    int status = ConfirmationID::Generate(argv[1], result, 0, "", false);
    if (status) {
        std::fprintf(stderr, "Generation error: %d\n", status);
        return status;
    }
    std::puts(result);
    return 0;
}
