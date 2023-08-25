import os
import shutil
import argparse

def main(environment_name):
    base_dir = f"../../logs/{environment_name}/"
    for root_dir, dirs, _ in os.walk(base_dir):
        for dir_name in dirs:
            tmp_dir_path = os.path.join(root_dir, dir_name, "tmp")
            if os.path.exists(tmp_dir_path):
                # Iterating through dynamically-named temporary directories within 'tmp'
                for dynamic_tmp_folder in os.listdir(tmp_dir_path):
                    dynamic_tmp_path = os.path.join(tmp_dir_path, dynamic_tmp_folder)
                    for item in os.listdir(dynamic_tmp_path):
                        source_path = os.path.join(dynamic_tmp_path, item)
                        dest_path = os.path.join(root_dir, dir_name, item)
                        if os.path.isdir(source_path):
                            if not os.path.exists(dest_path):
                                os.makedirs(dest_path)
                            for sub_item in os.listdir(source_path):
                                sub_source = os.path.join(source_path, sub_item)
                                sub_dest = os.path.join(dest_path, sub_item)
                                shutil.move(sub_source, sub_dest)
                        else:
                            shutil.move(source_path, dest_path)
                    shutil.rmtree(dynamic_tmp_path)
                shutil.rmtree(tmp_dir_path)

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Restructure directories.")
    parser.add_argument("environment_name", type=str, help="Name of the environment")
    args = parser.parse_args()
    main(args.environment_name)
