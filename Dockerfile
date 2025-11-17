# Use an official Python runtime as a parent image
FROM python:3.9-slim
# Set the working directory in the container
WORKDIR /usr/src/app

ENV USER_ID=99
ENV GROUP_ID=100

COPY parse_m3u.py /usr/src/app/parse_m3u.py
COPY requirements.txt /usr/src/app/requirements.txt

RUN pip install --no-cache-dir -r requirements.txt
RUN chown -R 99:100 /usr/src/app
USER 99:100

# Run the bash script when the container launches
CMD ["python", "/usr/src/app/parse_m3u.py"]
